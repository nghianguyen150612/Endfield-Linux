use gtk::prelude::*;

pub const STYLE: &str = include_str!("style.css");
const BRAND_MARK_SVG: &[u8] = include_bytes!("icons/endfield-mark.svg");

pub fn brand_image() -> (gtk::Image, bool) {
    let image = gtk::Image::from_icon_name("applications-engineering-symbolic");
    image.set_pixel_size(28);
    image.add_css_class("brand-mark");

    let bytes = gtk::glib::Bytes::from_static(BRAND_MARK_SVG);
    let loaded = match gtk::gdk::Texture::from_bytes(&bytes) {
        Ok(texture) => {
            image.set_paintable(Some(&texture));
            true
        }
        Err(_) => false,
    };
    (image, loaded)
}
