use std::collections::HashSet;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Extension, Path, Query};
use axum::routing::{get, post};
use axum::{Json, Router};
use bili_sync_entity::*;
use chrono::NaiveDateTime;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Expr, ExprTrait};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter,
    QueryOrder, Select, TransactionTrait, TryIntoModel,
};

use crate::api::error::InnerApiError;
use crate::api::helper::{update_page_download_status, update_video_download_status};
use crate::api::request::{
    DeleteVideosRequest, ExtractAudioRequest, ResetFilteredVideoStatusRequest, ResetVideoStatusRequest,
    UpdateFilteredVideoStatusRequest, UpdateVideoStatusRequest, VideosRequest,
};
use crate::api::response::{
    ClearAndResetVideoStatusResponse, DeleteVideosResponse, ExtractAudioResponse, PageInfo,
    ResetFilteredVideosResponse, ResetVideoResponse, SimplePageInfo, SimpleVideoInfo,
    UpdateFilteredVideoStatusResponse, UpdateVideoStatusResponse, VideoInfo, VideoResponse, VideosResponse,
};
use crate::api::wrapper::{ApiError, ApiResponse, ValidatedJson};
use crate::bilibili::BiliClient;
use crate::downloader::Downloader;
use crate::utils::status::{PageStatus, VideoStatus};

pub(super) fn router() -> Router {
    Router::new()
        .route("/videos", get(get_videos).delete(delete_videos))
        .route("/videos/extract-audio", post(extract_audio))
        .route("/videos/{id}", get(get_video))
        .route(
            "/videos/{id}/clear-and-reset-status",
            post(clear_and_reset_video_status),
        )
        .route("/videos/{id}/reset-status", post(reset_video_status))
        .route("/videos/{id}/update-status", post(update_video_status))
        .route("/videos/reset-status", post(reset_filtered_video_status))
        .route("/videos/update-status", post(update_filtered_video_status))
}

fn apply_created_at_filter(
    mut query: Select<video::Entity>,
    created_from: Option<NaiveDateTime>,
    created_to: Option<NaiveDateTime>,
) -> Select<video::Entity> {
    if let Some(created_from) = created_from {
        query = query.filter(
            Expr::cust_with_expr("datetime(?, 'localtime')", Expr::col(video::Column::CreatedAt))
                .gte(created_from.format("%Y-%m-%d %H:%M:%S").to_string()),
        );
    }
    if let Some(created_to) = created_to {
        query = query.filter(
            Expr::cust_with_expr("datetime(?, 'localtime')", Expr::col(video::Column::CreatedAt))
                .lte(created_to.format("%Y-%m-%d %H:%M:%S").to_string()),
        );
    }
    query
}

/// 列出视频的基本信息，支持根据视频来源筛选、名称查找和分页
pub async fn get_videos(
    Extension(db): Extension<DatabaseConnection>,
    Query(params): Query<VideosRequest>,
) -> Result<ApiResponse<VideosResponse>, ApiError> {
    let mut query = video::Entity::find();
    for (field, column) in [
        (params.collection, video::Column::CollectionId),
        (params.favorite, video::Column::FavoriteId),
        (params.submission, video::Column::SubmissionId),
        (params.watch_later, video::Column::WatchLaterId),
        (params.normal_video, video::Column::NormalVideoId),
    ] {
        if let Some(id) = field {
            query = query.filter(column.eq(id));
        }
    }
    if let Some(query_word) = params.query {
        query = query.filter(
            video::Column::Name
                .contains(&query_word)
                .or(video::Column::Bvid.contains(query_word)),
        );
    }
    if let Some(status_filter) = params.status_filter {
        query = query.filter(status_filter.to_video_query());
    }
    if let Some(validation_filter) = params.validation_filter {
        query = query.filter(validation_filter.to_video_query());
    }
    query = apply_created_at_filter(query, params.created_from, params.created_to);
    let total_count = query.clone().count(&db).await?;
    let (page, page_size) = if let (Some(page), Some(page_size)) = (params.page, params.page_size) {
        (page, page_size)
    } else {
        (0, 10)
    };
    Ok(ApiResponse::ok(VideosResponse {
        videos: query
            .order_by_desc(video::Column::Id)
            .into_partial_model::<VideoInfo>()
            .paginate(&db, page_size)
            .fetch_page(page)
            .await?,
        total_count,
    }))
}

pub async fn extract_audio(
    Extension(db): Extension<DatabaseConnection>,
    Extension(bili_client): Extension<Arc<BiliClient>>,
    Json(request): Json<ExtractAudioRequest>,
) -> Result<ApiResponse<ExtractAudioResponse>, ApiError> {
    let ids: HashSet<i32> = request.ids.into_iter().collect();
    if ids.is_empty() {
        return Err(InnerApiError::BadRequest("至少选择一个视频".to_string()).into());
    }
    let videos = video::Entity::find()
        .filter(video::Column::Id.is_in(ids.iter().copied()))
        .find_with_related(page::Entity)
        .all(&db)
        .await?;
    let downloader = Downloader::new(bili_client.client.clone());
    let mut extracted_count = 0;
    let mut warnings = Vec::new();
    for (video, pages) in videos {
        if video.single_page.is_none() {
            warnings.push(format!("视频「{}」缺少分页信息", video.name));
            continue;
        }
        for page in pages {
            let Some(video_path) = page.path.as_deref().filter(|path| !path.is_empty()).map(FsPath::new) else {
                warnings.push(format!("视频「{}」第 {} 页没有本地视频路径", video.name, page.pid));
                continue;
            };
            if !video_path.is_file() {
                warnings.push(format!(
                    "视频「{}」第 {} 页本地文件不存在：{}",
                    video.name,
                    page.pid,
                    video_path.display()
                ));
                continue;
            }
            let Some(file_stem) = video_path.file_stem() else {
                warnings.push(format!("视频「{}」第 {} 页本地文件名无效", video.name, page.pid));
                continue;
            };
            let audio_dir = video_path.parent().map(PathBuf::from);
            let Some(audio_dir) = audio_dir else {
                warnings.push(format!("视频「{}」第 {} 页本地路径无效", video.name, page.pid));
                continue;
            };
            let audio_path = audio_dir.join(file_stem).with_extension("m4a");
            match downloader.extract_audio(video_path, &audio_path).await {
                Ok(()) => extracted_count += 1,
                Err(error) => warnings.push(format!(
                    "视频「{}」第 {} 页音频提取失败：{:#}",
                    video.name, page.pid, error
                )),
            }
        }
    }
    Ok(ApiResponse::ok(ExtractAudioResponse {
        extracted_count,
        warnings,
    }))
}

pub async fn get_video(
    Path(id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
) -> Result<ApiResponse<VideoResponse>, ApiError> {
    let (video_info, pages_info) = tokio::try_join!(
        video::Entity::find_by_id(id).into_partial_model::<VideoInfo>().one(&db),
        page::Entity::find()
            .filter(page::Column::VideoId.eq(id))
            .order_by_asc(page::Column::Cid)
            .into_partial_model::<PageInfo>()
            .all(&db)
    )?;
    let Some(video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    Ok(ApiResponse::ok(VideoResponse {
        video: video_info,
        pages: pages_info,
    }))
}

pub async fn delete_videos(
    Extension(db): Extension<DatabaseConnection>,
    Json(request): Json<DeleteVideosRequest>,
) -> Result<ApiResponse<DeleteVideosResponse>, ApiError> {
    let ids: HashSet<i32> = request.ids.into_iter().collect();
    if ids.is_empty() {
        return Err(InnerApiError::BadRequest("至少选择一个视频".to_string()).into());
    }
    let videos = video::Entity::find()
        .filter(video::Column::Id.is_in(ids.iter().copied()))
        .find_with_related(page::Entity)
        .all(&db)
        .await?;
    let deleted_count = videos.len();
    let mut favorite_ids = HashSet::new();
    let mut collection_ids = HashSet::new();
    let mut submission_ids = HashSet::new();
    let mut watch_later_ids = HashSet::new();
    let mut normal_video_ids = HashSet::new();
    let page_paths = videos
        .into_iter()
        .flat_map(|(video, pages)| {
            if let Some(id) = video.favorite_id {
                favorite_ids.insert(id);
            }
            if let Some(id) = video.collection_id {
                collection_ids.insert(id);
            }
            if let Some(id) = video.submission_id {
                submission_ids.insert(id);
            }
            if let Some(id) = video.watch_later_id {
                watch_later_ids.insert(id);
            }
            if let Some(id) = video.normal_video_id {
                normal_video_ids.insert(id);
            }
            pages
                .into_iter()
                .filter_map(move |page| page.path.filter(|path| !path.is_empty()).map(|path| (video.id, path)))
        })
        .collect::<Vec<_>>();
    let txn = db.begin().await?;
    page::Entity::delete_many()
        .filter(page::Column::VideoId.is_in(ids.iter().copied()))
        .exec(&txn)
        .await?;
    video::Entity::delete_many()
        .filter(video::Column::Id.is_in(ids.iter().copied()))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    reset_source_latest_row_at(
        &db,
        &favorite_ids,
        &collection_ids,
        &submission_ids,
        &watch_later_ids,
        &normal_video_ids,
    )
    .await?;

    let mut warnings = Vec::new();
    for (id, path) in page_paths {
        if let Err(error) = remove_page_files(FsPath::new(&path)).await {
            warnings.push(format!("视频 {} 的本地文件「{}」删除失败：{:#}", id, path, error));
        }
    }
    Ok(ApiResponse::ok(DeleteVideosResponse {
        deleted_count,
        warnings,
    }))
}

async fn remove_page_files(path: &FsPath) -> Result<()> {
    let mut paths = vec![path.to_path_buf()];
    paths.push(path.with_extension("mp4"));
    paths.push(path.with_extension("m4a"));
    paths.push(path.with_file_name(format!(
        "{}-poster.jpg",
        path.file_stem().unwrap_or_default().to_string_lossy()
    )));
    paths.push(path.with_file_name(format!(
        "{}-fanart.jpg",
        path.file_stem().unwrap_or_default().to_string_lossy()
    )));
    paths.push(path.with_extension("nfo"));
    paths.push(path.with_extension("zh-CN.default.ass"));
    paths.push(path.with_extension("srt"));
    for candidate in paths {
        match tokio::fs::remove_file(&candidate).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

async fn reset_source_latest_row_at(
    db: &DatabaseConnection,
    favorite_ids: &HashSet<i32>,
    collection_ids: &HashSet<i32>,
    submission_ids: &HashSet<i32>,
    watch_later_ids: &HashSet<i32>,
    normal_video_ids: &HashSet<i32>,
) -> Result<()> {
    let reset_at = chrono::DateTime::UNIX_EPOCH.naive_utc();
    if !favorite_ids.is_empty() {
        favorite::Entity::update_many()
            .col_expr(favorite::Column::LatestRowAt, Expr::value(reset_at))
            .filter(favorite::Column::Id.is_in(favorite_ids.iter().copied()))
            .exec(db)
            .await?;
    }
    if !collection_ids.is_empty() {
        collection::Entity::update_many()
            .col_expr(collection::Column::LatestRowAt, Expr::value(reset_at))
            .filter(collection::Column::Id.is_in(collection_ids.iter().copied()))
            .exec(db)
            .await?;
    }
    if !submission_ids.is_empty() {
        submission::Entity::update_many()
            .col_expr(submission::Column::LatestRowAt, Expr::value(reset_at))
            .filter(submission::Column::Id.is_in(submission_ids.iter().copied()))
            .exec(db)
            .await?;
    }
    if !watch_later_ids.is_empty() {
        watch_later::Entity::update_many()
            .col_expr(watch_later::Column::LatestRowAt, Expr::value(reset_at))
            .filter(watch_later::Column::Id.is_in(watch_later_ids.iter().copied()))
            .exec(db)
            .await?;
    }
    if !normal_video_ids.is_empty() {
        normal_video::Entity::update_many()
            .col_expr(normal_video::Column::LatestRowAt, Expr::value(reset_at))
            .filter(normal_video::Column::Id.is_in(normal_video_ids.iter().copied()))
            .exec(db)
            .await?;
    }
    Ok(())
}

pub async fn reset_video_status(
    Path(id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
    Json(request): Json<ResetVideoStatusRequest>,
) -> Result<ApiResponse<ResetVideoResponse>, ApiError> {
    let (video_info, pages_info) = tokio::try_join!(
        video::Entity::find_by_id(id).into_partial_model::<VideoInfo>().one(&db),
        page::Entity::find()
            .filter(page::Column::VideoId.eq(id))
            .order_by_asc(page::Column::Cid)
            .into_partial_model::<PageInfo>()
            .all(&db)
    )?;
    let Some(mut video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    let resetted_pages_info = pages_info
        .into_iter()
        .filter_map(|mut page_info| {
            let mut page_status = PageStatus::from(page_info.download_status);
            if (request.force && page_status.force_reset_failed()) || page_status.reset_failed() {
                page_info.download_status = page_status.into();
                Some(page_info)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut video_status = VideoStatus::from(video_info.download_status);
    let mut video_resetted = (request.force && video_status.force_reset_failed()) || video_status.reset_failed();
    if !resetted_pages_info.is_empty() {
        video_status.set(4, 0); //  将“分页下载”重置为 0
        video_resetted = true;
    }
    let resetted_videos_info = if video_resetted {
        video_info.download_status = video_status.into();
        vec![&video_info]
    } else {
        vec![]
    };
    let resetted = !resetted_videos_info.is_empty() || !resetted_pages_info.is_empty();
    if resetted {
        let txn = db.begin().await?;
        if !resetted_videos_info.is_empty() {
            // 只可能有 1 个元素，所以不用 batch
            update_video_download_status::<VideoInfo>(&txn, &resetted_videos_info, None).await?;
        }
        if !resetted_pages_info.is_empty() {
            update_page_download_status(&txn, &resetted_pages_info, Some(500)).await?;
        }
        txn.commit().await?;
    }
    Ok(ApiResponse::ok(ResetVideoResponse {
        resetted,
        video: video_info,
        pages: resetted_pages_info,
    }))
}

pub async fn clear_and_reset_video_status(
    Path(id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
) -> Result<ApiResponse<ClearAndResetVideoStatusResponse>, ApiError> {
    let video_info = video::Entity::find_by_id(id).one(&db).await?;
    let Some(video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    let page_paths = page::Entity::find()
        .filter(page::Column::VideoId.eq(id))
        .all(&db)
        .await?
        .into_iter()
        .filter_map(|page| page.path.filter(|path| !path.is_empty()))
        .collect::<Vec<_>>();
    let txn = db.begin().await?;
    let mut video_info = video_info.into_active_model();
    video_info.single_page = Set(None);
    video_info.download_status = Set(0);
    video_info.valid = Set(true);
    let video_info = video_info.update(&txn).await?;
    page::Entity::delete_many()
        .filter(page::Column::VideoId.eq(id))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    let video_info = video_info.try_into_model()?;
    let mut warnings = Vec::new();
    for path in page_paths {
        if let Err(error) = remove_page_files(FsPath::new(&path)).await {
            warnings.push(format!("删除本地文件「{}」失败：{:#}", path, error));
        }
    }
    let warning = (!warnings.is_empty()).then(|| warnings.join("\n"));
    Ok(ApiResponse::ok(ClearAndResetVideoStatusResponse {
        warning,
        video: VideoInfo {
            id: video_info.id,
            bvid: video_info.bvid,
            name: video_info.name,
            upper_name: video_info.upper_name,
            valid: video_info.valid,
            should_download: video_info.should_download,
            download_status: video_info.download_status,
            collection_id: video_info.collection_id,
            favorite_id: video_info.favorite_id,
            submission_id: video_info.submission_id,
            watch_later_id: video_info.watch_later_id,
            normal_video_id: video_info.normal_video_id,
        },
    }))
}

pub async fn reset_filtered_video_status(
    Extension(db): Extension<DatabaseConnection>,
    Json(request): Json<ResetFilteredVideoStatusRequest>,
) -> Result<ApiResponse<ResetFilteredVideosResponse>, ApiError> {
    let mut query = video::Entity::find();
    for (field, column) in [
        (request.collection, video::Column::CollectionId),
        (request.favorite, video::Column::FavoriteId),
        (request.submission, video::Column::SubmissionId),
        (request.watch_later, video::Column::WatchLaterId),
        (request.normal_video, video::Column::NormalVideoId),
    ] {
        if let Some(id) = field {
            query = query.filter(column.eq(id));
        }
    }
    if let Some(query_word) = request.query {
        query = query.filter(
            video::Column::Name
                .contains(&query_word)
                .or(video::Column::Bvid.contains(query_word)),
        );
    }
    if let Some(status_filter) = request.status_filter {
        query = query.filter(status_filter.to_video_query());
    }
    if let Some(validation_filter) = request.validation_filter {
        query = query.filter(validation_filter.to_video_query());
    }
    query = apply_created_at_filter(query, request.created_from, request.created_to);
    let all_videos = query.into_partial_model::<SimpleVideoInfo>().all(&db).await?;
    let all_pages = page::Entity::find()
        .filter(page::Column::VideoId.is_in(all_videos.iter().map(|v| v.id)))
        .into_partial_model::<SimplePageInfo>()
        .all(&db)
        .await?;
    let resetted_pages_info = all_pages
        .into_iter()
        .filter_map(|mut page_info| {
            let mut page_status = PageStatus::from(page_info.download_status);
            if (request.force && page_status.force_reset_failed()) || page_status.reset_failed() {
                page_info.download_status = page_status.into();
                Some(page_info)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let video_ids_with_resetted_pages: HashSet<i32> = resetted_pages_info.iter().map(|page| page.video_id).collect();
    let resetted_videos_info = all_videos
        .into_iter()
        .filter_map(|mut video_info| {
            let mut video_status = VideoStatus::from(video_info.download_status);
            let mut video_resetted =
                (request.force && video_status.force_reset_failed()) || video_status.reset_failed();
            if video_ids_with_resetted_pages.contains(&video_info.id) {
                video_status.set(4, 0); // 将"分页下载"重置为 0
                video_resetted = true;
            }
            if video_resetted {
                video_info.download_status = video_status.into();
                Some(video_info)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let has_video_updates = !resetted_videos_info.is_empty();
    let has_page_updates = !resetted_pages_info.is_empty();
    if has_video_updates || has_page_updates {
        let txn = db.begin().await?;
        if has_video_updates {
            update_video_download_status(&txn, &resetted_videos_info, Some(500)).await?;
        }
        if has_page_updates {
            update_page_download_status(&txn, &resetted_pages_info, Some(500)).await?;
        }
        txn.commit().await?;
    }
    Ok(ApiResponse::ok(ResetFilteredVideosResponse {
        resetted: has_video_updates || has_page_updates,
        resetted_videos_count: resetted_videos_info.len(),
        resetted_pages_count: resetted_pages_info.len(),
    }))
}

pub async fn update_video_status(
    Path(id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
    ValidatedJson(request): ValidatedJson<UpdateVideoStatusRequest>,
) -> Result<ApiResponse<UpdateVideoStatusResponse>, ApiError> {
    let (video_info, mut pages_info) = tokio::try_join!(
        video::Entity::find_by_id(id).into_partial_model::<VideoInfo>().one(&db),
        page::Entity::find()
            .filter(page::Column::VideoId.eq(id))
            .order_by_asc(page::Column::Cid)
            .into_partial_model::<PageInfo>()
            .all(&db)
    )?;
    let Some(mut video_info) = video_info else {
        return Err(InnerApiError::NotFound(id).into());
    };
    let mut video_status = VideoStatus::from(video_info.download_status);
    for update in &request.video_updates {
        video_status.set(update.status_index, update.status_value);
    }
    video_info.download_status = video_status.into();
    let mut updated_pages_info = Vec::new();
    let mut page_id_map = pages_info
        .iter_mut()
        .map(|page| (page.id, page))
        .collect::<std::collections::HashMap<_, _>>();
    for page_update in &request.page_updates {
        if let Some(page_info) = page_id_map.remove(&page_update.page_id) {
            let mut page_status = PageStatus::from(page_info.download_status);
            for update in &page_update.updates {
                page_status.set(update.status_index, update.status_value);
            }
            page_info.download_status = page_status.into();
            updated_pages_info.push(page_info);
        }
    }
    let has_video_updates = !request.video_updates.is_empty();
    let has_page_updates = !updated_pages_info.is_empty();
    if has_video_updates || has_page_updates {
        let txn = db.begin().await?;
        if has_video_updates {
            update_video_download_status::<VideoInfo>(&txn, &[&video_info], None).await?;
        }
        if has_page_updates {
            update_page_download_status::<PageInfo>(&txn, &updated_pages_info, None).await?;
        }
        txn.commit().await?;
    }
    Ok(ApiResponse::ok(UpdateVideoStatusResponse {
        success: has_video_updates || has_page_updates,
        video: video_info,
        pages: pages_info,
    }))
}

pub async fn update_filtered_video_status(
    Extension(db): Extension<DatabaseConnection>,
    ValidatedJson(request): ValidatedJson<UpdateFilteredVideoStatusRequest>,
) -> Result<ApiResponse<UpdateFilteredVideoStatusResponse>, ApiError> {
    let mut query = video::Entity::find();
    for (field, column) in [
        (request.collection, video::Column::CollectionId),
        (request.favorite, video::Column::FavoriteId),
        (request.submission, video::Column::SubmissionId),
        (request.watch_later, video::Column::WatchLaterId),
        (request.normal_video, video::Column::NormalVideoId),
    ] {
        if let Some(id) = field {
            query = query.filter(column.eq(id));
        }
    }
    if let Some(query_word) = request.query {
        query = query.filter(
            video::Column::Name
                .contains(&query_word)
                .or(video::Column::Bvid.contains(query_word)),
        );
    }
    if let Some(status_filter) = request.status_filter {
        query = query.filter(status_filter.to_video_query());
    }
    if let Some(validation_filter) = request.validation_filter {
        query = query.filter(validation_filter.to_video_query());
    }
    query = apply_created_at_filter(query, request.created_from, request.created_to);
    let mut all_videos = query.into_partial_model::<SimpleVideoInfo>().all(&db).await?;
    let mut all_pages = page::Entity::find()
        .filter(page::Column::VideoId.is_in(all_videos.iter().map(|v| v.id)))
        .into_partial_model::<SimplePageInfo>()
        .all(&db)
        .await?;
    for video_info in all_videos.iter_mut() {
        let mut video_status = VideoStatus::from(video_info.download_status);
        for update in &request.video_updates {
            video_status.set(update.status_index, update.status_value);
        }
        video_info.download_status = video_status.into();
    }
    for page_info in all_pages.iter_mut() {
        let mut page_status = PageStatus::from(page_info.download_status);
        for update in &request.page_updates {
            page_status.set(update.status_index, update.status_value);
        }
        page_info.download_status = page_status.into();
    }
    let has_video_updates = !all_videos.is_empty();
    let has_page_updates = !all_pages.is_empty();
    if has_video_updates || has_page_updates {
        let txn = db.begin().await?;
        if has_video_updates {
            update_video_download_status(&txn, &all_videos, Some(500)).await?;
        }
        if has_page_updates {
            update_page_download_status(&txn, &all_pages, Some(500)).await?;
        }
        txn.commit().await?;
    }
    Ok(ApiResponse::ok(UpdateFilteredVideoStatusResponse {
        success: has_video_updates || has_page_updates,
        updated_videos_count: all_videos.len(),
        updated_pages_count: all_pages.len(),
    }))
}
