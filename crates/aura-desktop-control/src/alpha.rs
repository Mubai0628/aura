//! The Shell's images of icons the way a PNG holds them: colours not premultiplied by their
//! alpha.

/// Makes B, G, R, A pixels whose colours are premultiplied by their alpha straight. An image with
/// colours but no alpha at all (an old icon drawn without one) is made opaque instead of
/// invisible.
pub fn straighten(pixels: &mut [u8]) {
    let no_alpha = pixels.chunks_exact(4).all(|pixel| pixel[3] == 0);
    if no_alpha && pixels.iter().any(|&byte| byte != 0) {
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = u8::MAX;
        }
        return;
    }
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                let straight = (u16::from(*channel) * 255 + alpha / 2) / alpha;
                *channel = u8::try_from(straight).unwrap_or(u8::MAX);
            }
        }
    }
}
