//! Native 메뉴에 전달하는 단색 글리프. OS 핸들과 GPU 텍스처는 소유하지 않는다.

use tasty_type_geometry::length::{LogicalPx, PhysicalPx};

/// 기존 아이콘 SVG와 앱 zoom 전 논리 크기. 색은 native 메뉴의 상태별 잉크를 따른다.
#[derive(Clone, Copy, Debug)]
pub struct MenuIcon {
    svg: &'static str,
    pub(super) size: LogicalPx,
}

impl MenuIcon {
    /// `tasty_icons::Icon.svg`와 `SIZING.icon_glyph_size_md` 같은 기존 토큰을 전달한다.
    /// 잘못된 SVG/크기는 표시할 때 경고하고 아이콘만 생략한다.
    pub const fn from_svg(svg: &'static str, size: LogicalPx) -> Self {
        Self { svg, size }
    }

    pub(super) fn rasterize(self, scale_factor: f32) -> anyhow::Result<IconRaster> {
        use resvg::usvg::TreeParsing as _;
        anyhow::ensure!(
            self.size.value().is_finite()
                && self.size > LogicalPx(0.0)
                && scale_factor.is_finite()
                && scale_factor > 0.0,
            "invalid native menu icon size or scale"
        );
        let physical = self.size.to_physical(scale_factor);
        // Cairo/GDI stride는 signed i32이므로 외부 API 변환 전에 범위를 확인한다.
        anyhow::ensure!(
            physical.value() < (i32::MAX / 4) as f32,
            "icon raster too large"
        );
        let pixels = physical.value().ceil() as u32;
        let physical = PhysicalPx(pixels as f32);
        let tree = resvg::usvg::Tree::from_str(self.svg, &Default::default())?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels)
            .ok_or_else(|| anyhow::anyhow!("cannot allocate native menu icon"))?;
        let transform = resvg::tiny_skia::Transform::from_scale(
            physical.value() / tree.size.width(),
            physical.value() / tree.size.height(),
        );
        resvg::Tree::from_usvg(&tree).render(transform, &mut pixmap.as_mut());
        // SVG의 RGB를 native 메뉴에 강제하지 않고 coverage만 사용한다.
        let alpha = pixmap.data().chunks_exact(4).map(|p| p[3]).collect();
        Ok(IconRaster { physical, alpha })
    }
}

pub(super) struct IconRaster {
    pub physical: PhysicalPx,
    pub alpha: Vec<u8>,
}

impl IconRaster {
    /// Cairo ARGB32/Win32 DIB 경계용 premultiplied BGRA (little endian).
    #[cfg(any(target_os = "linux", windows, test))]
    pub fn bgra(&self, rgb: [u8; 3]) -> Vec<u8> {
        self.alpha
            .iter()
            .flat_map(|&a| {
                let channel = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
                [channel(rgb[2]), channel(rgb[1]), channel(rgb[0]), a]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_menu::{MenuItem, MenuShortcut};
    use tasty_type_appearance::theme::SIZING;

    #[test]
    fn raster_tracks_backing_scale_and_preserves_coverage() {
        let icon = MenuIcon::from_svg(tasty_icons::EXTERNAL.svg, SIZING.icon_glyph_size_md);
        for scale in [1.0, 1.25, 2.0] {
            let raster = icon.rasterize(scale).unwrap();
            assert_eq!(
                raster.physical,
                PhysicalPx(icon.size.to_physical(scale).value().ceil())
            );
            let width = raster.physical.value() as usize;
            assert_eq!(raster.alpha.len(), width * width);
            assert!(raster.alpha.contains(&0));
            assert!(raster.alpha.iter().any(|a| *a > 0 && *a < 255));
            assert!(raster.alpha.iter().any(|a| *a > 200));
        }
    }

    #[test]
    fn rejects_invalid_svg_size_and_scale() {
        assert!(
            MenuIcon::from_svg("invalid", SIZING.icon_glyph_size_md)
                .rasterize(1.0)
                .is_err()
        );
        for size in [LogicalPx(0.0), LogicalPx(-1.0), LogicalPx(f32::NAN)] {
            assert!(
                MenuIcon::from_svg(tasty_icons::FOLDER_OPEN.svg, size)
                    .rasterize(1.0)
                    .is_err()
            );
        }
        let icon = MenuIcon::from_svg(tasty_icons::FOLDER_OPEN.svg, SIZING.icon_glyph_size_md);
        for scale in [0.0, -1.0, f32::INFINITY] {
            assert!(icon.rasterize(scale).is_err());
        }
    }

    #[test]
    fn premultiplies_once_in_bgra_order() {
        let raster = IconRaster {
            physical: PhysicalPx(2.0),
            alpha: vec![0, 128, 255],
        };
        assert_eq!(
            raster.bgra([200, 100, 50]),
            [0, 0, 0, 0, 25, 50, 100, 128, 50, 100, 200, 255]
        );
    }

    #[test]
    fn builder_preserves_the_existing_menu_contract() {
        assert!(MenuItem::new(1, "plain").icon.is_none());
        assert!(MenuItem::separator().icon.is_none());
        let shortcut = MenuShortcut {
            binding: "ctrl+a".into(),
            display: "Ctrl+A".into(),
        };
        let item = MenuItem::disabled(7, "open")
            .with_tooltip("reason")
            .with_shortcut(Some(shortcut.clone()))
            .with_icon(MenuIcon::from_svg(
                tasty_icons::EXTERNAL.svg,
                SIZING.icon_glyph_size_md,
            ));
        assert_eq!(item.id, 7);
        assert_eq!(item.label, "open");
        assert!(!item.enabled);
        assert_eq!(item.tooltip.as_deref(), Some("reason"));
        assert_eq!(item.shortcut, Some(shortcut));
        assert!(item.icon.is_some());
    }
}
