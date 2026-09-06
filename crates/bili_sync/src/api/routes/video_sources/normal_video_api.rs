use std::sync::Arc;

use axum::extract::Extension;
use bili_sync_entity::normal_video;
use sea_orm::ActiveValue::Set;

use crate::adapter::{NormalVideoInput, parse_normal_video_input};
use crate::api::request::InsertNormalVideoRequest;
use crate::api::wrapper::{ApiError, ApiResponse, ValidatedJson};
use crate::bilibili::{BiliClient, Video, VideoInfo};
use crate::config::VersionedConfig;
use sea_orm::{DatabaseConnection, EntityTrait};

pub async fn insert_normal_video(
    Extension(db): Extension<DatabaseConnection>,
    Extension(bili_client): Extension<Arc<BiliClient>>,
    ValidatedJson(request): ValidatedJson<InsertNormalVideoRequest>,
) -> Result<ApiResponse<bool>, ApiError> {
    let input = parse_normal_video_input(&request.video)?;
    let credential = &VersionedConfig::get().read().credential;
    let video = Video::new(bili_client.as_ref(), "", credential);
    let detail = match input {
        NormalVideoInput::Bvid(bvid) => {
            Video::new(bili_client.as_ref(), &bvid, credential)
                .get_view_info()
                .await?
        }
        NormalVideoInput::Aid(aid) => video.get_view_info_by_aid(aid).await?,
    };
    let VideoInfo::Detail { title, bvid, .. } = detail else {
        unreachable!()
    };
    normal_video::Entity::insert(normal_video::ActiveModel {
        bvid: Set(bvid),
        name: Set(title),
        path: Set(request.path),
        enabled: Set(false),
        ..Default::default()
    })
    .exec(&db)
    .await?;
    Ok(ApiResponse::ok(true))
}
