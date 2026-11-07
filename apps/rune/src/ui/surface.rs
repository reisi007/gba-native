pub(super) fn nearest_scale(rgba: &[u8], width: u32, height: u32, factor: u32) -> Vec<u8> {
    if factor <= 1 {
        return rgba.to_vec();
    }
    let scaled_width = (width * factor) as usize;
    let scaled_height = (height * factor) as usize;
    let mut output = vec![0; scaled_width * scaled_height * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let source = (y * width as usize + x) * 4;
            for dy in 0..factor as usize {
                for dx in 0..factor as usize {
                    let target =
                        ((y * factor as usize + dy) * scaled_width + x * factor as usize + dx) * 4;
                    output[target..target + 4].copy_from_slice(&rgba[source..source + 4]);
                }
            }
        }
    }
    output
}

pub(super) fn nearest_fit(
    rgba: &[u8],
    width: u32,
    height: u32,
    target_width: u32,
    target_height: u32,
) -> Vec<u8> {
    let mut output = vec![0; target_width as usize * target_height as usize * 4];
    for y in 0..target_height as usize {
        for x in 0..target_width as usize {
            let source_x = x * width as usize / target_width as usize;
            let source_y = y * height as usize / target_height as usize;
            let source = (source_y * width as usize + source_x) * 4;
            let target = (y * target_width as usize + x) * 4;
            output[target..target + 4].copy_from_slice(&rgba[source..source + 4]);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_fit_uses_sharp_source_samples() {
        let source = [1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255];
        assert_eq!(nearest_fit(&source, 2, 2, 1, 1), [1, 0, 0, 255]);
        assert_eq!(nearest_fit(&source, 2, 2, 4, 4).len(), 64);
    }
}
