//! Host outputs are bounded and validated before any image becomes a conversation artifact.
use aura_agent_domain::*;
use aura_domain_common::ArtifactId;
use base64::Engine as _;
use serde_json::Value;

pub(super) struct Image {
    pub id: ArtifactId,
    pub bytes: Vec<u8>,
}
pub(super) struct Checked {
    pub output: Value,
    pub images: Vec<Image>,
}

fn image(raw: MediaImageV1, max_side: u32, total: &mut usize) -> anyhow::Result<Image> {
    anyhow::ensure!(
        raw.media_type == "image/jpeg"
            && raw.width > 0
            && raw.height > 0
            && raw.width.max(raw.height) <= max_side
            && raw.data_base64.len() <= MAX_MEDIA_IMAGE_BYTES.div_ceil(3) * 4,
        "media.image_invalid: 视频帧的类型、尺寸或大小不可用。"
    );
    let bytes = base64::engine::general_purpose::STANDARD.decode(raw.data_base64)?;
    *total += bytes.len();
    anyhow::ensure!(
        bytes.starts_with(&[0xff, 0xd8, 0xff])
            && bytes.len() <= MAX_MEDIA_IMAGE_BYTES
            && *total <= MAX_MEDIA_IMAGES_BYTES,
        "media.image_invalid: 视频帧不是有效大小的JPEG。"
    );
    Ok(Image { id: ArtifactId::new(), bytes })
}

pub(super) fn checked(
    action: &MediaHostActionV1,
    result: MediaHostCompletedV1,
    size: u64,
    shown: bool,
) -> anyhow::Result<Checked> {
    anyhow::ensure!(
        serde_json::to_vec(&result)?.len() <= MAX_MEDIA_CONTROL_BYTES,
        "media.result_too_large: 视频结果超过传输上限。"
    );
    match (action, result) {
        (MediaHostActionV1::Probe, MediaHostCompletedV1::Probe(probe)) => {
            anyhow::ensure!(
                probe.duration.is_finite()
                    && probe.duration > 0.0
                    && probe.width > 0
                    && probe.height > 0
                    && probe.byte_length == size
                    && probe.frame_rate.is_none_or(|rate| rate.is_finite() && rate > 0.0)
                    && [&probe.container, &probe.codec].into_iter().all(|value| value
                        .as_ref()
                        .is_none_or(
                            |text| text.len() <= 128 && !text.chars().any(char::is_control)
                        )),
                "media.probe_invalid: 解码器返回了不可用的视频元数据。"
            );
            Ok(Checked { output: serde_json::to_value(probe)?, images: vec![] })
        }
        (
            MediaHostActionV1::Frames { times, count, sheet, max_side },
            MediaHostCompletedV1::Frames(raw),
        ) => {
            let expected = times.as_ref().map_or_else(|| count.unwrap_or(0) as usize, Vec::len);
            anyhow::ensure!(
                expected > 0
                    && expected <= MAX_MEDIA_FRAMES
                    && raw.frames.len() == expected
                    && raw.sheet.is_some() == *sheet,
                "media.frames_invalid: 解码器没有返回要求的帧数或拼图。"
            );
            let mut total = 0;
            let mut images = vec![];
            let mut frames = vec![];
            for (index, frame) in raw.frames.into_iter().enumerate() {
                anyhow::ensure!(
                    frame.requested_time.is_finite()
                        && frame.requested_time >= 0.0
                        && frame.actual_time.is_finite()
                        && frame.actual_time >= 0.0
                        && times.as_ref().is_none_or(|times| (times[index] - frame.requested_time)
                            .abs()
                            < 0.000_001)
                        && frame.image.is_some() != *sheet,
                    "media.frames_invalid: 视频帧时间或图像与请求不符。"
                );
                let image_artifact_id =
                    frame.image.map(|raw| image(raw, *max_side, &mut total)).transpose()?.and_then(
                        |image| {
                            if shown {
                                let id = image.id;
                                images.push(image);
                                Some(id)
                            } else {
                                None
                            }
                        },
                    );
                frames.push(MediaFrameResult {
                    requested_time: frame.requested_time,
                    actual_time: frame.actual_time,
                    image_artifact_id,
                });
            }
            let sheet_artifact_id =
                raw.sheet.map(|raw| image(raw, *max_side, &mut total)).transpose()?.and_then(
                    |image| {
                        if shown {
                            let id = image.id;
                            images.push(image);
                            Some(id)
                        } else {
                            None
                        }
                    },
                );
            let output = MediaFramesResult {
                frames,
                sheet_artifact_id,
                shown_to_model: shown,
                note: if shown {
                    "这些帧作为图片随本条结果发给你；可以直接查看。"
                } else {
                    "当前模型不接收图片，无法看到视频帧；这里只提供每帧时间，请如实说明这一限制。"
                }
                .into(),
            };
            Ok(Checked { output: serde_json::to_value(output)?, images })
        }
        _ => anyhow::bail!("media.result_invalid: 视频解码结果与操作不符。"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn frame() -> MediaImageV1 {
        MediaImageV1 {
            media_type: "image/jpeg".into(),
            data_base64: "/9j/4AAQSkZJRg==".into(),
            width: 32,
            height: 24,
        }
    }
    fn action() -> MediaHostActionV1 {
        MediaHostActionV1::Frames {
            times: Some(vec![0.5]),
            count: None,
            sheet: false,
            max_side: 1024,
        }
    }
    fn result() -> MediaHostCompletedV1 {
        MediaHostCompletedV1::Frames(MediaFramesHostResultV1 {
            frames: vec![MediaHostFrameV1 {
                requested_time: 0.5,
                actual_time: 0.48,
                image: Some(frame()),
            }],
            sheet: None,
        })
    }
    #[test]
    fn frames_for_a_model_without_images_keep_only_times_and_an_explicit_limitation() {
        let hidden = checked(&action(), result(), 1, false).unwrap();
        assert!(hidden.images.is_empty());
        assert_eq!(hidden.output["frames"][0]["actual_time"], 0.48);
        assert_eq!(hidden.output["shown_to_model"], false);
        assert!(hidden.output["frames"][0]["image_artifact_id"].is_null());
        let shown = checked(&action(), result(), 1, true).unwrap();
        assert_eq!(shown.images.len(), 1);
        assert_eq!(shown.output["shown_to_model"], true);
    }
    #[test]
    fn wrong_frame_times_or_oversized_images_are_refused_before_storage() {
        let MediaHostCompletedV1::Frames(mut wrong) = result() else { unreachable!() };
        wrong.frames[0].requested_time = 1.0;
        assert!(checked(&action(), MediaHostCompletedV1::Frames(wrong), 1, true).is_err());
        let MediaHostCompletedV1::Frames(mut wrong) = result() else { unreachable!() };
        wrong.frames[0].image.as_mut().unwrap().data_base64 = "A".repeat(MAX_MEDIA_IMAGE_BYTES * 2);
        assert!(checked(&action(), MediaHostCompletedV1::Frames(wrong), 1, true).is_err());
    }
}
