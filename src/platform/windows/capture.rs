/// Screenshot capture using GDI BitBlt.

use crate::platform::windows::apps;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Capture the main window of a process as PNG bytes.
pub fn screenshot(pid: u32) -> Result<Vec<u8>, String> {
    let hwnd = find_main_window(pid)?;

    let mut rect = Default::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
    }

    let width = (rect.right - rect.left) as i32;
    let height = (rect.bottom - rect.top) as i32;
    if width <= 0 || height <= 0 {
        return Err("Window has zero size".into());
    }

    unsafe {
        let hdc_screen = GetDC(Some(hwnd));
        if hdc_screen.is_invalid() {
            return Err("GetDC failed".into());
        }

        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let hbm = CreateCompatibleBitmap(hdc_screen, width, height);
        let old = SelectObject(hdc_mem, hbm);

        // Copy window content
        let _ = BitBlt(hdc_mem, 0, 0, width, height, Some(hdc_screen), 0, 0, SRCCOPY);

        // Read bitmap bits
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        GetDIBits(
            hdc_mem,
            hbm,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        // Cleanup GDI
        SelectObject(hdc_mem, old);
        let _ = DeleteObject(hbm);
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(Some(hwnd), hdc_screen);

        // Convert BGRA to RGBA
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2); // B <-> R
        }

        // Encode as PNG using the image crate
        let img = image::RgbaImage::from_raw(width as u32, height as u32, pixels)
            .ok_or("Failed to create image buffer")?;

        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        image::ImageEncoder::write_image(
            encoder,
            &img,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("PNG encode failed: {e}"))?;

        Ok(png_bytes)
    }
}

fn find_main_window(target_pid: u32) -> Result<HWND, String> {
    use windows::Win32::Foundation::{BOOL, LPARAM};

    let mut result: Option<HWND> = None;

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut (u32, Option<HWND>));
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == data.0 && IsWindowVisible(hwnd).as_bool() {
            data.1 = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut data = (target_pid, None);
    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut data as *mut _ as isize),
        );
    }
    data.1.ok_or_else(|| format!("No window found for PID {target_pid}"))
}
