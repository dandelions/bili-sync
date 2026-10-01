use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use anyhow::{Result, bail, ensure};
use bili_sync_entity::rule::Rule;
use bili_sync_entity::*;
use futures::Stream;
use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{OnConflict, SimpleExpr};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, QueryOrder, Unchanged};

use crate::adapter::{_ActiveModel, VideoSource, VideoSourceEnum};
use crate::bilibili::{BiliClient, Credential, PageInfo, Video, VideoInfo};
use crate::utils::status::VideoStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalVideoInput {
    Bvid(String),
    Aid(i64),
}

pub fn parse_normal_video_input(input: &str) -> Result<NormalVideoInput> {
    let input = input.trim();
    let token = if let Some(path) = input
        .strip_prefix("https://www.bilibili.com/video/")
        .or_else(|| input.strip_prefix("http://www.bilibili.com/video/"))
        .or_else(|| input.strip_prefix("https://bilibili.com/video/"))
        .or_else(|| input.strip_prefix("http://bilibili.com/video/"))
        .or_else(|| input.strip_prefix("www.bilibili.com/video/"))
        .or_else(|| input.strip_prefix("bilibili.com/video/"))
    {
        path.split(['?', '#', '/']).next().unwrap_or_default()
    } else {
        input.split(['?', '#', '/']).next().unwrap_or_default()
    };
    if token.starts_with("BV") && token.len() >= 12 && token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Ok(NormalVideoInput::Bvid(token.to_string()));
    }
    let digits = token
        .strip_prefix("av")
        .or_else(|| token.strip_prefix("AV"))
        .or_else(|| token.strip_prefix("aid"))
        .or_else(|| token.strip_prefix("AID"))
        .unwrap_or(token);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(NormalVideoInput::Aid(digits.parse()?));
    }
    bail!("请输入裸 BV、av/aid 数字或 bilibili.com/video/BV 地址")
}

impl VideoSource for normal_video::Model {
    fn display_name(&self) -> Cow<'static, str> {
        format!("普通视频「{}」", self.name).into()
    }

    fn filter_expr(&self) -> SimpleExpr {
        video::Column::NormalVideoId.eq(self.id)
    }

    fn set_relation_id(&self, video_model: &mut video::ActiveModel) {
        video_model.normal_video_id = Set(Some(self.id));
    }

    fn path(&self) -> &Path {
        Path::new(self.path.as_str())
    }

    fn get_latest_row_at(&self) -> DateTime {
        self.latest_row_at
    }

    fn update_latest_row_at(&self, datetime: DateTime) -> _ActiveModel {
        _ActiveModel::NormalVideo(normal_video::ActiveModel {
            id: Unchanged(self.id),
            latest_row_at: Set(datetime),
            ..Default::default()
        })
    }

    fn should_take(
        &self,
        _idx: usize,
        _release_datetime: &chrono::DateTime<chrono::Utc>,
        _latest_row_at: &chrono::DateTime<chrono::Utc>,
    ) -> bool {
        true
    }

    fn rule(&self) -> &Option<Rule> {
        &self.rule
    }

    fn filter_option(&self) -> &Option<Json> {
        &self.filter_option
    }

    fn video_name(&self) -> Option<&str> {
        self.video_name.as_deref().filter(|s| !s.trim().is_empty())
    }

    async fn refresh<'a>(
        self,
        bili_client: &'a BiliClient,
        credential: &'a Credential,
        connection: &'a DatabaseConnection,
    ) -> Result<(
        VideoSourceEnum,
        Pin<Box<dyn Stream<Item = Result<VideoInfo>> + Send + 'a>>,
    )> {
        let video = Video::new(bili_client, &self.bvid, credential);
        let detail = video.get_view_info().await?;
        let VideoInfo::Detail {
            title,
            bvid,
            cover,
            ctime,
            pubtime,
            pages,
            ..
        } = detail
        else {
            bail!("普通视频详情接口返回了非 Detail 数据")
        };
        ensure!(bvid == self.bvid, "video bvid mismatch: {} != {}", bvid, self.bvid);
        normal_video::Entity::update(normal_video::ActiveModel {
            id: Unchanged(self.id),
            name: Set(title.clone()),
            ..Default::default()
        })
        .exec(connection)
        .await?;

        // 检查数据库中是否已存在该视频记录；如果已存在，则自动进行增量分 P 检测与更新
        let existing_video = video::Entity::find()
            .filter(video::Column::NormalVideoId.eq(self.id))
            .one(connection)
            .await?;

        if let Some(existing_video) = existing_video {
            let existing_pages = page::Entity::find()
                .filter(page::Column::VideoId.eq(existing_video.id))
                .order_by_asc(page::Column::Pid)
                .all(connection)
                .await?;

            let existing_pids: HashSet<i32> = existing_pages.iter().map(|p| p.pid).collect();
            let new_pages: Vec<PageInfo> = pages
                .iter()
                .filter(|p| !existing_pids.contains(&p.page))
                .cloned()
                .collect();
            let now_single_page = pages.len() == 1;
            let was_single_page = existing_video.single_page.unwrap_or(existing_pages.len() <= 1);

            // 同步已存在分 P 的元数据变更（如标题或 cid 修改）
            for p in &pages {
                if let Some(existing) = existing_pages.iter().find(|ep| ep.pid == p.page) {
                    if existing.name != p.name || existing.cid != p.cid {
                        let mut ep_active: page::ActiveModel = existing.clone().into();
                        ep_active.name = Set(p.name.clone());
                        ep_active.cid = Set(p.cid);
                        ep_active.update(connection).await?;
                    }
                }
            }

            if !new_pages.is_empty() {
                // 如果之前作为单 P 录入与下载，现在变为了多 P，则平滑迁移 P1 的本地文件名与数据库路径
                if was_single_page && !now_single_page {
                    if let Err(e) = migrate_single_page_files(&existing_pages, connection).await {
                        tracing::warn!("平滑迁移单 P 文件为多 P 命名结构时发生警告: {:#}", e);
                    }
                }

                // 批量插入新增分 P 记录
                let new_active_pages = new_pages
                    .into_iter()
                    .map(|p| p.into_active_model(existing_video.id))
                    .collect::<Vec<_>>();
                let new_pages_count = new_active_pages.len();

                for chunk in new_active_pages.chunks(200) {
                    page::Entity::insert_many(chunk.to_vec())
                        .on_conflict(
                            OnConflict::columns([page::Column::VideoId, page::Column::Pid])
                                .do_nothing()
                                .to_owned(),
                        )
                        .exec(connection)
                        .await?;
                }

                // 重置视频下载状态中的“分页下载”任务（第 4 位），set(4, 0) 会自动清除最高位完成标记
                let mut video_status = VideoStatus::from(existing_video.download_status);
                video_status.set(4, 0);

                let mut video_active: video::ActiveModel = existing_video.into();
                video_active.name = Set(title.clone());
                video_active.cover = Set(cover.clone());
                video_active.single_page = Set(Some(now_single_page));
                video_active.download_status = Set(video_status.into());
                video_active.valid = Set(true);
                video_active.should_download = Set(true);
                video_active.update(connection).await?;

                tracing::info!(
                    "普通视频「{}」自动增量检测到 {} 个新增分 P，已加入下载队列",
                    title,
                    new_pages_count
                );
            }
        }

        let summary = VideoInfo::Collection {
            bvid,
            cover,
            ctime,
            pubtime,
        };
        Ok((self.into(), Box::pin(futures::stream::once(async move { Ok(summary) }))))
    }

    async fn delete_from_db(self, conn: &impl ConnectionTrait) -> Result<()> {
        self.delete(conn).await?;
        Ok(())
    }
}

/// 当视频由单 P 变为多 P 时，将原先下载的 P1 本地文件平滑迁移至包含 ` - P01` 的文件名结构
async fn migrate_single_page_files(
    existing_pages: &[page::Model],
    connection: &DatabaseConnection,
) -> Result<()> {
    let Some(p1_page) = existing_pages.iter().find(|p| p.pid == 1) else {
        return Ok(());
    };
    let Some(old_path_str) = &p1_page.path else {
        return Ok(());
    };
    if old_path_str.is_empty() {
        return Ok(());
    }
    let old_media_path = PathBuf::from(old_path_str);
    let Some(parent_dir) = old_media_path.parent() else {
        return Ok(());
    };
    let Some(old_file_stem) = old_media_path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(());
    };
    if old_file_stem.ends_with(" - P01") || old_file_stem.ends_with(" - P1") {
        return Ok(());
    }
    let new_file_stem = format!("{} - P01", old_file_stem);

    let rename_candidates = [
        (format!("{}.mp4", old_file_stem), format!("{}.mp4", new_file_stem)),
        (format!("{}.m4a", old_file_stem), format!("{}.m4a", new_file_stem)),
        (format!("{}.mp3", old_file_stem), format!("{}.mp3", new_file_stem)),
        (format!("{}.m4b", old_file_stem), format!("{}.m4b", new_file_stem)),
        (format!("{}.nfo", old_file_stem), format!("{}.nfo", new_file_stem)),
        (
            format!("{}.zh-CN.default.ass", old_file_stem),
            format!("{}.zh-CN.default.ass", new_file_stem),
        ),
        (format!("{}.srt", old_file_stem), format!("{}.srt", new_file_stem)),
        (
            format!("{}-poster.jpg", old_file_stem),
            format!("{}-poster.jpg", new_file_stem),
        ),
        (
            format!("{}-fanart.jpg", old_file_stem),
            format!("{}-fanart.jpg", new_file_stem),
        ),
    ];

    let mut media_migrated = false;
    let old_media_ext = old_media_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");
    let new_media_path = parent_dir.join(format!("{}.{}", new_file_stem, old_media_ext));

    for (old_name, new_name) in rename_candidates {
        let old_file = parent_dir.join(&old_name);
        let new_file = parent_dir.join(&new_name);
        if old_file.exists() && !new_file.exists() {
            if let Err(e) = tokio::fs::rename(&old_file, &new_file).await {
                tracing::warn!(
                    "迁移单 P 文件 {} -> {} 失败: {:#}",
                    old_file.display(),
                    new_file.display(),
                    e
                );
            } else if old_file == old_media_path {
                media_migrated = true;
            }
        }
    }

    if media_migrated || new_media_path.exists() {
        let mut p1_active: page::ActiveModel = p1_page.clone().into();
        p1_active.path = Set(Some(new_media_path.to_string_lossy().to_string()));
        p1_active.update(connection).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_supported_inputs() {
        assert_eq!(
            parse_normal_video_input("BV1abCDefGhI").unwrap(),
            NormalVideoInput::Bvid("BV1abCDefGhI".into())
        );
        assert_eq!(
            parse_normal_video_input("https://www.bilibili.com/video/BV1abCDefGhI?p=2").unwrap(),
            NormalVideoInput::Bvid("BV1abCDefGhI".into())
        );
        assert_eq!(parse_normal_video_input("av123").unwrap(), NormalVideoInput::Aid(123));
        assert_eq!(parse_normal_video_input("aid456").unwrap(), NormalVideoInput::Aid(456));
        assert_eq!(parse_normal_video_input("789").unwrap(), NormalVideoInput::Aid(789));
    }
}
