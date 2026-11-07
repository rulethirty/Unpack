//! PNG decoding uses the system libpng simplified API in a worker. It avoids
//! launching image helper processes or weakening Flatpak's sandbox. libpng is
//! already a GTK dependency; no new image codec implementation is introduced.
use super::*;
use std::{
    ffi::{c_char, c_int, c_void, CStr},
    path::Path,
};

pub(crate) struct Pixels {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

#[repr(C)]
struct PngImage {
    opaque: *mut c_void,
    version: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
    colormap_entries: u32,
    warning_or_error: u32,
    message: [c_char; 64],
}
#[link(name = "png16")]
unsafe extern "C" {
    fn png_image_begin_read_from_memory(
        image: *mut PngImage,
        data: *const c_void,
        size: usize,
    ) -> c_int;
    fn png_image_finish_read(
        image: *mut PngImage,
        background: *const c_void,
        buffer: *mut c_void,
        stride: i32,
        colormap: *mut c_void,
    ) -> c_int;
    fn png_image_free(image: *mut PngImage);
}
struct Decoder(PngImage);
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            png_image_free(&mut self.0);
        }
    }
}
impl Decoder {
    fn error(&self) -> String {
        unsafe { CStr::from_ptr(self.0.message.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }
}

pub(super) fn decode(path: &Path, bytes: Vec<u8>) -> Result<Pixels, String> {
    unpack_core::identity::validate_icon(path, &bytes).map_err(|e| e.to_string())?;
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("png"))
    {
        // SAFETY: zero initialization and version=1 are mandated by libpng's
        // stable png_image API. The input is bounded and lives through decoding.
        let mut image = Decoder(unsafe { std::mem::zeroed::<PngImage>() });
        image.0.version = 1;
        if unsafe {
            png_image_begin_read_from_memory(&mut image.0, bytes.as_ptr().cast(), bytes.len())
        } == 0
        {
            return Err(image.error());
        }
        if image.0.width == 0
            || image.0.height == 0
            || image.0.width > 4096
            || image.0.height > 4096
        {
            return Err("Icon dimensions exceed limits".into());
        }
        image.0.format = 3; // PNG_FORMAT_RGBA: COLOR | ALPHA, 8 bits/channel.
        let mut rgba = vec![0u8; image.0.width as usize * image.0.height as usize * 4];
        if unsafe {
            png_image_finish_read(
                &mut image.0,
                std::ptr::null(),
                rgba.as_mut_ptr().cast(),
                0,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(image.error());
        }
        return Ok(Pixels {
            width: image.0.width as i32,
            height: image.0.height as i32,
            rgba,
        });
    }
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        96,
        96,
        true,
        None::<&gio::Cancellable>,
    )
    .map_err(|e| e.to_string())?;
    let pixbuf = pixbuf
        .add_alpha(false, 0, 0, 0)
        .map_err(|e| e.to_string())?;
    let source = pixbuf.read_pixel_bytes();
    let width = pixbuf.width();
    let height = pixbuf.height();
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in 0..height as usize {
        let offset = row * pixbuf.rowstride() as usize;
        rgba.extend_from_slice(&source.as_ref()[offset..offset + width as usize * 4]);
    }
    Ok(Pixels {
        width,
        height,
        rgba,
    })
}

pub(super) fn texture(pixels: Pixels) -> gtk::gdk::MemoryTexture {
    gtk::gdk::MemoryTexture::new(
        pixels.width,
        pixels.height,
        gtk::gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(pixels.rgba),
        pixels.width as usize * 4,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_decoding_is_bounded_and_checks_corruption() {
        let bytes = vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 48, 109,
            121, 242, 31, 0, 5, 44, 2, 157, 207, 67, 233, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ];
        let pixels = decode(Path::new("app.png"), bytes.clone()).unwrap();
        assert_eq!((pixels.width, pixels.height), (1, 1));
        assert_eq!(pixels.rgba, [53, 132, 228, 255]);
        let mut corrupted = bytes;
        corrupted[50] ^= 255;
        assert!(decode(Path::new("app.png"), corrupted).is_err());
        assert!(decode(Path::new("app.png"), b"bad".to_vec()).is_err());
    }
}
