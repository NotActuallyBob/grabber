use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{GenericImage, ImageFormat, RgbaImage};
use std::io::Cursor;
use xcap::Monitor;

#[derive(serde::Serialize)]
struct ScreenCapture {
    data_url: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[tauri::command]
fn capture_screen() -> Result<ScreenCapture, String> {
    let monitors = Monitor::all().map_err(|error| error.to_string())?;
    if monitors.is_empty() {
        return Err("No monitor was found".to_string());
    }

    let mut captures = Vec::new();
    for monitor in monitors {
        let x = monitor.x().map_err(|error| error.to_string())?;
        let y = monitor.y().map_err(|error| error.to_string())?;
        let screenshot = monitor
            .capture_image()
            .map_err(|error| error.to_string())?;
        captures.push((x, y, screenshot));
    }

    let min_x = captures.iter().map(|(x, _, _)| *x).min().unwrap_or(0);
    let min_y = captures.iter().map(|(_, y, _)| *y).min().unwrap_or(0);
    let max_x = captures
        .iter()
        .map(|(x, _, image)| *x + image.width() as i32)
        .max()
        .unwrap_or(0);
    let max_y = captures
        .iter()
        .map(|(_, y, image)| *y + image.height() as i32)
        .max()
        .unwrap_or(0);
    let width = (max_x - min_x) as u32;
    let height = (max_y - min_y) as u32;
    let mut screenshot = RgbaImage::new(width, height);

    for (x, y, monitor_image) in captures {
        screenshot
            .copy_from(
                &monitor_image,
                (x - min_x) as u32,
                (y - min_y) as u32,
            )
            .map_err(|error| error.to_string())?;
    }

    let mut png = Cursor::new(Vec::new());
    screenshot
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| error.to_string())?;

    Ok(ScreenCapture {
        data_url: format!(
            "data:image/png;base64,{}",
            STANDARD.encode(png.into_inner())
        ),
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![capture_screen])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
