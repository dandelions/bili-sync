use std::borrow::Cow;
use std::path::Path;
use std::pin::Pin;

use anyhow::{Result, bail, ensure};
use bili_sync_entity::rule::Rule;
use bili_sync_entity::*;
use futures::Stream;
use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{ConnectionTrait, DatabaseConnection, Unchanged};

use crate::adapter::{_ActiveModel, VideoSource, VideoSourceEnum};
use crate::bilibili::{BiliClient, Credential, Video, VideoInfo};

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
            ..
        } = detail
        else {
            bail!("普通视频详情接口返回了非 Detail 数据")
        };
        ensure!(bvid == self.bvid, "video bvid mismatch: {} != {}", bvid, self.bvid);
        normal_video::Entity::update(normal_video::ActiveModel {
            id: Unchanged(self.id),
            name: Set(title),
            ..Default::default()
        })
        .exec(connection)
        .await?;
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
