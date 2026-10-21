//! GTK 메뉴 행 안의 단색 글리프. 색과 배율은 그리는 시점의 native 상태를 읽는다.

use gtk::prelude::*;
use tasty_type_geometry::length::LogicalPx;

pub(super) fn widget(icon: crate::native_menu::MenuIcon, item: &gtk::MenuItem) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    let side = icon.size.value().ceil() as i32;
    area.set_size_request(side, side);
    area.set_valign(gtk::Align::Center);
    // 메뉴 테마가 정한 간격을 쓰며 앱의 별도 시각 값을 도입하지 않는다.
    let padding = LogicalPx(item.style_context().padding(item.state_flags()).left as f32);
    area.set_margin_end(padding.value() as i32);
    let item = item.downgrade();
    area.connect_draw(move |area, cr| {
        let Some(item) = item.upgrade() else {
            return gtk::glib::Propagation::Proceed;
        };
        let result = (|| -> anyhow::Result<()> {
            let raster = icon.rasterize(area.scale_factor() as f32)?;
            let side = raster.physical.value() as i32;
            let stride = gtk::cairo::Format::ARgb32.stride_for_width(side as u32)?;
            let surface = gtk::cairo::ImageSurface::create_for_data(
                raster.bgra([255, 255, 255]),
                gtk::cairo::Format::ARgb32,
                side,
                side,
                stride,
            )?;
            // GTK backing 배율로 저장한 픽셀을 논리 좌표로 그린다.
            let scale = f64::from(area.scale_factor());
            surface.set_device_scale(scale, scale);
            let color = item.style_context().color(item.state_flags());
            cr.set_source_rgba(color.red(), color.green(), color.blue(), color.alpha());
            cr.mask_surface(&surface, 0.0, 0.0)?;
            Ok(())
        })();
        if let Err(e) = result {
            tracing::warn!("native menu icon: {e}");
        }
        gtk::glib::Propagation::Proceed
    });
    area
}
