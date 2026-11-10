use super::{
    ACCENT, BEZEL, BODY, BUTTON, DIM, HEIGHT, PANEL, PRESSED, RgbaImage, TEXT, TouchLayout, WIDTH,
    blank, blit_scaled, draw_text, fill, finish, text_width,
};

/// Windowed GBA-shell composition with the guest image at 2x and touch controls.
pub fn shell(screen: &RgbaImage, pressed: u16) -> RgbaImage {
    shell_with_controls(screen, pressed, true)
}

/// Shared shell composition with optional visible host-side touch controls.
pub fn shell_with_controls(screen: &RgbaImage, pressed: u16, show_controls: bool) -> RgbaImage {
    shell_with_controls_label(screen, pressed, show_controls, "SYNTHETIC")
}

/// Shared shell with an explicit content label for non-synthetic sessions.
pub fn shell_with_controls_label(
    screen: &RgbaImage,
    pressed: u16,
    show_controls: bool,
    footer: &str,
) -> RgbaImage {
    let mut rgba = blank();
    fill(&mut rgba, 0, 0, WIDTH, HEIGHT, BODY);
    fill(&mut rgba, 0, 0, WIDTH, 26, PANEL);
    draw_text(&mut rgba, 16, 8, "GBA-NATIVE", ACCENT, 2);
    draw_text(&mut rgba, 600, 8, "WINDOWED", DIM, 2);

    let layout = TouchLayout::landscape();
    let (sx, sy, sw, sh) = layout.screen.pixels(WIDTH, HEIGHT);
    fill(&mut rgba, sx - 8, sy - 8, sw + 16, sh + 16, BEZEL);
    blit_scaled(&mut rgba, screen, sx, sy, sw, sh);

    if show_controls {
        for (button, rect) in &layout.controls {
            let (bx, by, bw, bh) = rect.pixels(WIDTH, HEIGHT);
            let active = pressed & button.bit() != 0;
            fill(
                &mut rgba,
                bx,
                by,
                bw,
                bh,
                if active { PRESSED } else { BUTTON },
            );
            let label = button.name();
            let tw = text_width(label, 2);
            draw_text(
                &mut rgba,
                bx + bw.saturating_sub(tw) / 2,
                by + bh / 2 - 5,
                label,
                if active { BODY } else { TEXT },
                2,
            );
        }
    }

    draw_text(
        &mut rgba,
        16,
        566,
        "F1 SETTINGS  F2 SAVES  F11 FULLSCREEN",
        DIM,
        2,
    );
    draw_text(&mut rgba, 600, 566, footer, ACCENT, 2);
    finish(rgba, screen.guest_budget())
}

/// Minimal one-game handheld composition: no window chrome or menu hints.
///
/// It keeps the landscape screen and control rectangles used by
/// [`super::hit_test`], so pointer input resolves exactly as in the windowed
/// shell. This is presentation only and never executes guest code.
pub fn game_shell(screen: &RgbaImage, pressed: u16, show_controls: bool) -> RgbaImage {
    let mut rgba = blank();
    fill(&mut rgba, 0, 0, WIDTH, HEIGHT, BODY);
    let layout = TouchLayout::landscape();
    let (sx, sy, sw, sh) = layout.screen.pixels(WIDTH, HEIGHT);
    fill(&mut rgba, sx - 8, sy - 8, sw + 16, sh + 16, BEZEL);
    blit_scaled(&mut rgba, screen, sx, sy, sw, sh);
    if show_controls {
        for (button, rect) in &layout.controls {
            let (bx, by, bw, bh) = rect.pixels(WIDTH, HEIGHT);
            let active = pressed & button.bit() != 0;
            fill(
                &mut rgba,
                bx,
                by,
                bw,
                bh,
                if active { PRESSED } else { BUTTON },
            );
            let label = button.name();
            let tw = text_width(label, 2);
            draw_text(
                &mut rgba,
                bx + bw.saturating_sub(tw) / 2,
                by + bh / 2 - 5,
                label,
                if active { BODY } else { TEXT },
                2,
            );
        }
    }
    finish(rgba, screen.guest_budget())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::image::demo_image;

    #[test]
    fn touch_control_visibility_changes_only_the_host_shell_image() {
        let demo = demo_image().expect("synthetic demo image");
        let with_controls = shell_with_controls(&demo, 0, true);
        let without_controls = shell_with_controls(&demo, 0, false);
        assert_ne!(with_controls.rgba(), without_controls.rgba());
        assert_eq!(shell(&demo, 0), with_controls);
        assert_eq!(without_controls.guest_budget(), demo.guest_budget());
    }
}
