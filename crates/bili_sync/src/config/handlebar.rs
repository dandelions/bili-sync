use std::sync::LazyLock;

use anyhow::Result;
use handlebars::{Context, Handlebars, Helper, HelperDef, HelperResult, Output, RenderContext};

use crate::config::versioned_cache::VersionedCache;
use crate::config::{Config, PathSafeTemplate};
use crate::notifier::{Notifier, webhook_template_content, webhook_template_key};

pub static TEMPLATE: LazyLock<VersionedCache<handlebars::Handlebars<'static>>> =
    LazyLock::new(|| VersionedCache::new(create_template).expect("Failed to create handlebars template"));

fn create_template(config: &Config) -> Result<handlebars::Handlebars<'static>> {
    create_template_with_video_name(config, &config.video_name)
}

pub fn create_template_with_video_name(config: &Config, video_name: &str) -> Result<handlebars::Handlebars<'static>> {
    let mut handlebars = handlebars::Handlebars::new();
    handlebars.register_helper("truncate", Box::new(TruncateHelper));
    handlebars.path_safe_register("video", video_name.to_owned())?;
    handlebars.path_safe_register("page", config.page_name.clone())?;
    handlebars.path_safe_register("favorite_default_path", config.favorite_default_path.clone())?;
    handlebars.path_safe_register("collection_default_path", config.collection_default_path.clone())?;
    handlebars.path_safe_register("submission_default_path", config.submission_default_path.clone())?;
    if let Some(notifiers) = &config.notifiers {
        for notifier in notifiers.iter() {
            if let Notifier::Webhook { url, template, .. } = notifier {
                handlebars.register_template_string(&webhook_template_key(url), webhook_template_content(template))?;
            }
        }
    }
    Ok(handlebars)
}

#[derive(Clone, Copy)]
pub struct TruncateHelper;

impl HelperDef for TruncateHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        let s = match h.param(0) {
            Some(v) => match v.value() {
                serde_json::Value::String(s) => s.as_str(),
                _ => "",
            },
            None => "",
        };

        let total_chars = s.chars().count();
        let (start, len) = parse_truncate_params(h, total_chars);
        let start = start.min(total_chars);
        let chars = s.chars().skip(start);
        let result: String = match len {
            Some(l) => chars.take(l).collect(),
            None => chars.collect(),
        };

        out.write(&result)?;
        Ok(())
    }
}

/// 解析截取参数：返回 (start_offset, Option<length>)
///
/// 支持以下语法：
/// 1. 负数：`{{truncate title -10}}` -> 截取第 10 个字符之后的数据（跳过前 10 个字符，取剩余全部内容）
/// 2. 命名参数：
///    `{{truncate title start=10}}` 或 `{{truncate title offset=10}}` -> 截取第 10 个字符之后的数据
///    `{{truncate title 20 start=10}}` -> 从第 10 个字符开始，截取 20 个字符
///    `{{truncate title len=20 start=10}}` -> 从第 10 个字符开始，截取 20 个字符
/// 3. 双数字位置参数：
///    `{{truncate title 20 10}}` -> 从第 10 个字符开始，截取 20 个字符
///    `{{truncate title 0 10}}` -> 从第 10 个字符开始，截取全部后续字符
/// 4. 切片语法：
///    `{{truncate title "10:"}}` -> 截取第 10 个字符之后的数据
///    `{{truncate title ":10"}}` -> 截取前 10 个字符
///    `{{truncate title "5:15"}}` -> 截取字符索引 5..15
/// 5. 传统单正数位置参数：
///    `{{truncate title 10}}` -> 截取前 10 个字符
fn parse_truncate_params(h: &Helper<'_>, total_chars: usize) -> (usize, Option<usize>) {
    // 优先检查命名参数 (start / offset / from / len / length)
    let hash_start = h
        .hash_get("start")
        .or_else(|| h.hash_get("offset"))
        .or_else(|| h.hash_get("from"))
        .and_then(|v| v.value().as_i64());

    let hash_len = h
        .hash_get("len")
        .or_else(|| h.hash_get("length"))
        .and_then(|v| v.value().as_i64());

    if hash_start.is_some() || hash_len.is_some() {
        let start = match hash_start {
            Some(st) if st >= 0 => st as usize,
            Some(st) => total_chars.saturating_sub((-st) as usize),
            None => 0,
        };
        let len = match hash_len.or_else(|| h.param(1).and_then(|v| v.value().as_i64())) {
            Some(l) if l > 0 => Some(l as usize),
            _ => None,
        };
        return (start, len);
    }

    if let Some(p1) = h.param(1) {
        // 如果 p1 是切片语法字符串，如 "10:", ":10", "5:15"
        if let Some(slice_str) = p1.value().as_str() {
            if let Some((start_part, end_part)) = slice_str.split_once(':') {
                let start = if start_part.trim().is_empty() {
                    0
                } else if let Ok(s) = start_part.trim().parse::<i64>() {
                    if s >= 0 {
                        s as usize
                    } else {
                        total_chars.saturating_sub((-s) as usize)
                    }
                } else {
                    0
                };

                let end = if end_part.trim().is_empty() {
                    None
                } else if let Ok(e) = end_part.trim().parse::<i64>() {
                    if e >= 0 {
                        Some(e as usize)
                    } else {
                        Some(total_chars.saturating_sub((-e) as usize))
                    }
                } else {
                    None
                };

                let len = match end {
                    Some(e) if e >= start => Some(e - start),
                    Some(_) => Some(0),
                    None => None,
                };
                return (start, len);
            }
        }

        // p1 作为数值
        if let Some(p1_num) = p1.value().as_i64() {
            if let Some(p2) = h.param(2).and_then(|v| v.value().as_i64()) {
                // 双位置参数：truncate s len start
                let start = if p2 >= 0 {
                    p2 as usize
                } else {
                    total_chars.saturating_sub((-p2) as usize)
                };
                let len = if p1_num > 0 { Some(p1_num as usize) } else { None };
                return (start, len);
            } else {
                // 只有一个数值参数：
                // 正数：截取前 p1_num 个字符
                // 负数：截取第 |p1_num| 个字符之后的数据（跳过前 |p1_num| 个字符）
                if p1_num >= 0 {
                    return (0, Some(p1_num as usize));
                } else {
                    return ((-p1_num) as usize, None);
                }
            }
        }
    }

    (0, None)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_template_usage() {
        let mut template = handlebars::Handlebars::new();
        template.register_helper("truncate", Box::new(TruncateHelper));
        let _ = template.path_safe_register("video", "test{{bvid}}test");
        let _ = template.path_safe_register("test_truncate", "哈哈，{{ truncate title 30 }}");
        let _ = template.path_safe_register("test_path_unix", "{{ truncate title 7 }}/test/a");
        let _ = template.path_safe_register("test_path_windows", r"{{ truncate title 7 }}\\test\\a");
        let _ = template.path_safe_register("test_truncate_after", "{{ truncate title -6 }}");
        let _ = template.path_safe_register("test_truncate_start", "{{ truncate title start=6 }}");
        let _ = template.path_safe_register("test_truncate_offset", "{{ truncate title offset=6 }}");
        let _ = template.path_safe_register("test_truncate_slice", "{{ truncate title '6:' }}");
        let _ = template.path_safe_register("test_truncate_len_start", "{{ truncate title 4 6 }}");
        #[cfg(not(windows))]
        {
            assert_eq!(
                template
                    .path_safe_render("test_path_unix", &json!({"title": "关注/永雏塔菲喵"}))
                    .unwrap(),
                "关注_永雏塔菲/test/a"
            );
            assert_eq!(
                template
                    .path_safe_render("test_path_windows", &json!({"title": "关注/永雏塔菲喵"}))
                    .unwrap(),
                "关注_永雏塔菲_test_a"
            );
        }
        #[cfg(windows)]
        {
            assert_eq!(
                template
                    .path_safe_render("test_path_unix", &json!({"title": "关注/永雏塔菲喵"}))
                    .unwrap(),
                "关注_永雏塔菲_test_a"
            );
            assert_eq!(
                template
                    .path_safe_render("test_path_windows", &json!({"title": "关注/永雏塔菲喵"}))
                    .unwrap(),
                r"关注_永雏塔菲\\test\\a"
            );
        }
        assert_eq!(
            template
                .path_safe_render("video", &json!({"bvid": "BV1b5411h7g7"}))
                .unwrap(),
            "testBV1b5411h7g7test"
        );
        assert_eq!(
            template
                .path_safe_render(
                    "test_truncate",
                    &json!({"title": "你说得对，但是 Rust 是由 Mozilla 自主研发的一款全新的编译期格斗游戏。\
                    编译将发生在一个被称作「Cargo」的构建系统中。在这里，被引用的指针将被授予「生命周期」之力，导引对象安全。\
                    你将扮演一位名为「Rustacean」的神秘角色，在与「Rustc」的搏斗中邂逅各种骨骼惊奇的傲娇报错。\
                    征服她们、通过编译同时，逐步发掘「C++」程序崩溃的真相。"})
                )
                .unwrap(),
            "哈哈，你说得对，但是 Rust 是由 Mozilla 自主研发的一"
        );
        // 截取指定字符之后的数据
        let test_title = json!({"title": "【官方双语】真实标题来了"});
        assert_eq!(
            template.path_safe_render("test_truncate_after", &test_title).unwrap(),
            "真实标题来了"
        );
        assert_eq!(
            template.path_safe_render("test_truncate_start", &test_title).unwrap(),
            "真实标题来了"
        );
        assert_eq!(
            template.path_safe_render("test_truncate_offset", &test_title).unwrap(),
            "真实标题来了"
        );
        assert_eq!(
            template.path_safe_render("test_truncate_slice", &test_title).unwrap(),
            "真实标题来了"
        );
        assert_eq!(
            template.path_safe_render("test_truncate_len_start", &test_title).unwrap(),
            "真实标题"
        );
    }
}
