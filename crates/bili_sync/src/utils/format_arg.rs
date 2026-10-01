use serde_json::json;

pub fn video_format_args(video_model: &bili_sync_entity::video::Model, time_format: &str) -> serde_json::Value {
    let is_single_page = video_model.single_page.unwrap_or(true);
    json!({
        "bvid": &video_model.bvid,
        "title": &video_model.name,
        "upper_name": &video_model.upper_name,
        "upper_mid": &video_model.upper_id,
        "ptitle": &video_model.name,
        "pid": 1,
        "p": 1,
        "pid_pad": "01",
        "p_pad": "01",
        "page_pad": "01",
        "P_pad": "P01",
        "p_pad_lower": "p01",
        "is_single_page": is_single_page,
        "pubtime": &video_model.pubtime.and_utc().format(time_format).to_string(),
        "fav_time": &video_model.favtime.and_utc().format(time_format).to_string(),
    })
}

pub fn page_format_args(
    video_model: &bili_sync_entity::video::Model,
    page_model: &bili_sync_entity::page::Model,
    time_format: &str,
) -> serde_json::Value {
    let pid = page_model.pid;
    let pid_pad = format!("{:0>2}", pid);
    let p_pad = pid_pad.clone();
    let p_pad_upper = format!("P{:0>2}", pid);
    let p_pad_lower = format!("p{:0>2}", pid);
    let is_single_page = video_model.single_page.unwrap_or(true);
    json!({
        "bvid": &video_model.bvid,
        "title": &video_model.name,
        "upper_name": &video_model.upper_name,
        "upper_mid": &video_model.upper_id,
        "ptitle": &page_model.name,
        "pid": pid,
        "p": pid,
        "pid_pad": pid_pad,
        "p_pad": p_pad,
        "page_pad": format!("{:0>2}", pid),
        "P_pad": p_pad_upper,
        "p_pad_lower": p_pad_lower,
        "is_single_page": is_single_page,
        "pubtime": video_model.pubtime.and_utc().format(time_format).to_string(),
        "fav_time": video_model.favtime.and_utc().format(time_format).to_string(),
    })
}
