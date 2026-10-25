//! Media release facts. Keep each published identity and fingerprint immutable.
use super::PublishedRevision;

pub const PUBLISHED_MEDIA: &[PublishedRevision] = &[
    PublishedRevision {
        name: "media.probe",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202M",
        instance: "tin_01M2DG020980080021AN94202M",
        fingerprint: "80907a98563568a9b105b22f331df1543d686bf8e7af89152d1b0594a311bd20",
    },
    PublishedRevision {
        name: "media.frames",
        revision: 1,
        tool: "tol_01M2DG020980080021AN94202N",
        instance: "tin_01M2DG020980080021AN94202N",
        fingerprint: "58562e3d43dbeda9ab8e7c9c0d3a80e16e6e5ab5ca52565300ee9662753e833a",
    },
];
