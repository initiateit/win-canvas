//! Win-Canvas: An infinite canvas for managing open windows.
//!
//! Press Ctrl+Alt+Space to toggle the canvas overlay.
//! Features: wallpaper background, fade-in animation, persistent layout.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod canvas;
mod dwm;
mod enumerate;
mod hotkey;
mod input;
mod state;
mod window;

use std::cell::RefCell;
use std::fs;
use std::io::Write;

use std::result::Result::Ok;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use canvas::{Canvas, SourceInfo};
use dwm::Thumbnail;

// GDI+ token
static mut GDIPLUS_TOKEN: usize = 0;

// Animation constants
const TIMER_FADE_IN: usize = 1;
const TIMER_SCROLL_ANIM: usize = 2;
const TIMER_TEXT_FADE_IN: usize = 3;
const ANIM_INTERVAL_MS: u32 = 16;
const ANIM_STEPS: u32 = 10;
const TEXT_ANIM_STEPS: u32 = 10;
const TARGET_ALPHA: u8 = 255;

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// Simple debug logger
fn log_debug(msg: &str) {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let log_dir = std::path::PathBuf::from(&appdata).join("win-canvas");
    let _ = fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("debug.log");
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(f, "{}", msg);
    }
    #[cfg(debug_assertions)]
    eprintln!("{}", msg);
}

struct AppState {
    canvas: Canvas,
    thumbnails: Vec<Thumbnail>,
    visible: bool,
    canvas_hwnd: HWND,
    drag_moved: bool,
    click_target: Option<usize>,
    bg_bitmap: HBITMAP,
    anim_step: u32,
    anim_active: bool,
    text_anim_step: u32,
    text_anim_active: bool,
    current_alpha: u8,
}

impl AppState {
    fn new(screen_w: i32, screen_h: i32) -> Self {
        Self {
            canvas: Canvas::new(screen_w, screen_h),
            thumbnails: Vec::new(),
            visible: false,
            canvas_hwnd: HWND::default(),
            drag_moved: false,
            click_target: None,
            bg_bitmap: HBITMAP::default(),
            anim_step: 0,
            anim_active: false,
            text_anim_step: 0,
            text_anim_active: false,
            current_alpha: 0,
        }
    }

    fn refresh(&mut self) {
        self.thumbnails.clear();
        self.canvas.windows.clear();

        let windows = enumerate::enumerate_windows();
        log_debug(&format!("Enumerated {} windows", windows.len()));

        let mut source_infos = Vec::new();

        for winfo in &windows {
            if winfo.hwnd == self.canvas_hwnd {
                continue;
            }
            match Thumbnail::register(self.canvas_hwnd, winfo.hwnd) {
                Ok(thumb) => {
                    let idx = self.thumbnails.len();
                    source_infos.push(SourceInfo {
                        thumb_index: idx,
                        width: thumb.source_width,
                        height: thumb.source_height,
                        title: winfo.title.clone(),
                        icon: winfo.icon,
                    });
                    self.thumbnails.push(thumb);
                }
                Err(e) => {
                    log_debug(&format!(
                        "Failed to register thumbnail for '{}': {:?}",
                        winfo.title, e
                    ));
                }
            }
        }

        log_debug(&format!("Registered {} thumbnails", self.thumbnails.len()));

        let saved = state::load_state();
        self.canvas.layout_grid(&source_infos, saved.as_ref());
        self.update_all_thumbnails();
    }

    fn update_all_thumbnails(&self) {
        let scale = if self.anim_active {
            let t = self.anim_step as f64 / ANIM_STEPS as f64;
            0.92 + 0.08 * ease_out(t)
        } else {
            1.0
        };

        for cw in &self.canvas.windows {
            if cw.thumb_index < self.thumbnails.len() {
                let rect = self.canvas.canvas_to_screen_rect(cw, scale);
                if rect.right > 0
                    && rect.bottom > 0
                    && rect.left < self.canvas.screen_w
                    && rect.top < self.canvas.screen_h
                {
                    let _ = self.thumbnails[cw.thumb_index]
                        .update(rect, self.current_alpha, false);
                } else {
                    let _ = self.thumbnails[cw.thumb_index].hide();
                }
            }
        }
    }

    fn toggle(&mut self) {
        log_debug(&format!("Toggle called, visible={}", self.visible));
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    fn show(&mut self) {
        log_debug("show() called");
        self.visible = true;

        // Capture the current screen as background
        if !self.bg_bitmap.0.is_null() {
            window::free_bitmap(self.bg_bitmap);
            self.bg_bitmap = HBITMAP::default();
        }
        self.bg_bitmap =
            window::capture_screen(self.canvas.screen_w, self.canvas.screen_h);
        log_debug(&format!("Screen captured: bitmap={:?}", self.bg_bitmap.0));

        self.current_alpha = 0;
        window::set_window_alpha(self.canvas_hwnd, 0);

        self.refresh();
        window::show_canvas(self.canvas_hwnd);

        // Start fade-in animation
        self.anim_step = 0;
        self.anim_active = true;
        unsafe {
            SetTimer(self.canvas_hwnd, TIMER_FADE_IN, ANIM_INTERVAL_MS, None);
        }
        log_debug("show() complete, animation started");
    }

    fn hide(&mut self) {
        log_debug("hide() called");
        let saved = self.canvas.to_saved_state();
        state::save_state(&saved);

        self.visible = false;
        self.anim_active = false;
        self.text_anim_active = false;
        unsafe {
            let _ = KillTimer(self.canvas_hwnd, TIMER_FADE_IN);
            let _ = KillTimer(self.canvas_hwnd, TIMER_TEXT_FADE_IN);
        }

        for thumb in &self.thumbnails {
            let _ = thumb.hide();
        }
        window::hide_canvas(self.canvas_hwnd);
    }

    fn tick_animation(&mut self) {
        self.anim_step += 1;
        if self.anim_step >= ANIM_STEPS {
            self.anim_step = ANIM_STEPS;
            self.anim_active = false;
            unsafe {
                let _ = KillTimer(self.canvas_hwnd, TIMER_FADE_IN);
                
                // Start text fade in
                self.text_anim_active = true;
                self.text_anim_step = 0;
                let _ = SetTimer(self.canvas_hwnd, TIMER_TEXT_FADE_IN, ANIM_INTERVAL_MS, None);
            }
        }

        let t = self.anim_step as f64 / ANIM_STEPS as f64;
        let eased = ease_out(t);
        self.current_alpha = (TARGET_ALPHA as f64 * eased) as u8;

        window::set_window_alpha(self.canvas_hwnd, self.current_alpha);
        self.update_all_thumbnails();

        unsafe {
            let _ = InvalidateRect(self.canvas_hwnd, None, true);
        }
    }

    fn tick_text_animation(&mut self) {
        self.text_anim_step += 1;
        if self.text_anim_step >= TEXT_ANIM_STEPS {
            self.text_anim_step = TEXT_ANIM_STEPS;
            self.text_anim_active = false;
            unsafe {
                let _ = KillTimer(self.canvas_hwnd, TIMER_TEXT_FADE_IN);
            }
        }

        unsafe {
            let _ = InvalidateRect(self.canvas_hwnd, None, true);
        }
    }
}

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = RefCell::new(None);
}

fn with_state<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut AppState) -> R,
{
    APP_STATE.with(|cell| {
        if let Ok(mut opt) = cell.try_borrow_mut() {
            opt.as_mut().map(|state| f(state))
        } else {
            // State is currently borrowed (re-entrant call), skip
            None
        }
    })
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            if wparam.0 as i32 == hotkey::HOTKEY_TOGGLE_CANVAS {
                with_state(|s| s.toggle());
            }
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == TIMER_FADE_IN {
                with_state(|s| s.tick_animation());
            } else if wparam.0 == TIMER_TEXT_FADE_IN {
                with_state(|s| s.tick_text_animation());
            } else if wparam.0 == TIMER_SCROLL_ANIM {
                with_state(|s| {
                    if s.canvas.update_scroll_animation() {
                        s.update_all_thumbnails();
                        let _ = InvalidateRect(hwnd, None, true);
                    } else {
                        // Animation complete, kill timer
                        unsafe {
                            let _ = KillTimer(hwnd, TIMER_SCROLL_ANIM);
                        }
                    }
                });
            }
            LRESULT(0)
        }

        WM_KEYDOWN => {
            let vk = wparam.0 as u32;
            if vk == 0x1B {
                // ESC - close the window
                with_state(|s| s.hide());
            } else if vk == 0x25 || vk == 0x26 {
                // Left or Up arrow - previous window
                with_state(|s| {
                    s.canvas.prev_window();
                    unsafe {
                        let _ = SetTimer(hwnd, TIMER_SCROLL_ANIM, ANIM_INTERVAL_MS, None);
                    }
                    s.update_all_thumbnails();
                    let _ = InvalidateRect(hwnd, None, true);
                });
            } else if vk == 0x27 || vk == 0x28 {
                // Right or Down arrow - next window
                with_state(|s| {
                    s.canvas.next_window();
                    unsafe {
                        let _ = SetTimer(hwnd, TIMER_SCROLL_ANIM, ANIM_INTERVAL_MS, None);
                    }
                    s.update_all_thumbnails();
                    let _ = InvalidateRect(hwnd, None, true);
                });
            } else if vk == 0x0D {
                // Enter key - activate the selected window
                with_state(|s| {
                    if let Some(idx) = s.canvas.get_active_window() {
                        if idx < s.canvas.windows.len() {
                            let ti = s.canvas.windows[idx].thumb_index;
                            if ti < s.thumbnails.len() {
                                let target = s.thumbnails[ti].source_hwnd;
                                s.hide();
                                window::activate_window(target);
                            }
                        }
                    }
                });
            }
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            let (x, y) = input::mouse_coords(lparam.0);
            with_state(|s| {
                let hit = s.canvas.hit_test(x, y);
                s.click_target = hit;
                s.drag_moved = false;
                // Set active window and scroll to it
                if let Some(idx) = hit {
                    s.canvas.set_active_window(idx);
                    unsafe {
                        let _ = SetTimer(hwnd, TIMER_SCROLL_ANIM, ANIM_INTERVAL_MS, None);
                    }
                }
                // Window dragging disabled - only canvas panning with right-click
            });
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            with_state(|s| {
                if !s.drag_moved {
                    if let Some(idx) = s.click_target {
                        if idx < s.canvas.windows.len() {
                            let ti = s.canvas.windows[idx].thumb_index;
                            if ti < s.thumbnails.len() {
                                let target = s.thumbnails[ti].source_hwnd;
                                s.hide();
                                window::activate_window(target);
                            }
                        }
                    }
                }
                s.canvas.end_drag();
                s.click_target = None;
                let _ = ReleaseCapture();
                s.update_all_thumbnails();
                let _ = InvalidateRect(hwnd, None, true);
            });
            LRESULT(0)
        }

        WM_RBUTTONDOWN => {
            let (x, y) = input::mouse_coords(lparam.0);
            with_state(|s| {
                s.canvas.start_pan(x, y);
                SetCapture(hwnd);
            });
            LRESULT(0)
        }

        WM_RBUTTONUP => {
            with_state(|s| {
                s.canvas.end_pan();
                let _ = ReleaseCapture();
                s.update_all_thumbnails();
                let _ = InvalidateRect(hwnd, None, true);
            });
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let (x, y) = input::mouse_coords(lparam.0);
            with_state(|s| {
                if s.canvas.drag_target.is_some() {
                    s.drag_moved = true;
                    s.canvas.update_drag(x, y);
                    s.update_all_thumbnails();
                    let _ = InvalidateRect(hwnd, None, true);
                } else if s.canvas.panning {
                    s.canvas.update_pan(x, y);
                    s.update_all_thumbnails();
                    let _ = InvalidateRect(hwnd, None, true);
                }
            });
            LRESULT(0)
        }

        WM_MOUSEWHEEL => {
            let (x, y) = input::mouse_coords(lparam.0);
            let delta = input::wheel_delta(wparam.0);
            let ctrl_pressed = (wparam.0 & 0x0008) != 0; // MK_CONTROL

            with_state(|s| {
                if ctrl_pressed {
                    // Ctrl+Wheel = zoom
                    let mut pt = POINT {
                        x: x as i32,
                        y: y as i32,
                    };
                    let _ = ScreenToClient(hwnd, &mut pt);
                    s.canvas.zoom_at(pt.x as f64, pt.y as f64, delta);
                } else {
                    // Wheel without Ctrl = navigate through windows
                    if delta > 0.0 {
                        s.canvas.prev_window();
                    } else {
                        s.canvas.next_window();
                    }
                    unsafe {
                        let _ = SetTimer(hwnd, TIMER_SCROLL_ANIM, ANIM_INTERVAL_MS, None);
                    }
                }
                s.update_all_thumbnails();
                let _ = InvalidateRect(hwnd, None, true);
            });
            LRESULT(0)
        }

        WM_ERASEBKGND => {
            // Don't erase - we'll handle all painting in WM_PAINT to prevent flicker
            LRESULT(1)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            with_state(|s| {
                // Use double buffering: draw to off-screen bitmap first
                let hdc_buffer = CreateCompatibleDC(hdc);
                let hbm_buffer = CreateCompatibleBitmap(hdc, s.canvas.screen_w, s.canvas.screen_h);
                let _old_buffer = SelectObject(hdc_buffer, hbm_buffer);

                // Draw background first (captured screen)
                if !s.bg_bitmap.0.is_null() {
                    let hdc_mem = CreateCompatibleDC(hdc_buffer);
                    let old = SelectObject(hdc_mem, s.bg_bitmap);

                    // Draw the captured screen to buffer
                    let _ = BitBlt(
                        hdc_buffer, 0, 0,
                        s.canvas.screen_w, s.canvas.screen_h,
                        hdc_mem, 0, 0, SRCCOPY,
                    );
                    SelectObject(hdc_mem, old);
                    let _ = DeleteDC(hdc_mem);
                }

                // Now draw borders and text to hdc_buffer
                SetBkMode(hdc_buffer, TRANSPARENT);
                SetTextColor(hdc_buffer, COLORREF(0x00E0E0E0));

                let font_name = window::wide_string("Segoe UI");
                let font = CreateFontW(
                    24, 0, 0, 0, 700, 0, 0, 0, 0, 0, 0, 0, 0,
                    PCWSTR(font_name.as_ptr()),
                );
                let old_font = SelectObject(hdc_buffer, font);

                let scale = if s.anim_active {
                    let t = s.anim_step as f64 / ANIM_STEPS as f64;
                    0.92 + 0.08 * ease_out(t)
                } else {
                    1.0
                };

                // Draw borders to hdc_buffer
                for (idx, cw) in s.canvas.windows.iter().enumerate() {
                    let rect = s.canvas.canvas_to_screen_rect(cw, scale);
                    let is_active = s.canvas.get_active_window() == Some(idx);

                    // Use GDI+ for anti-aliased rounded corners
                    unsafe {
                        let mut graphics: *mut GpGraphics = std::ptr::null_mut();
                        if GdipCreateFromHDC(hdc_buffer, &mut graphics as *mut _ as *mut _) == Status(0) {
                            let _ = GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);

                            let mut pen: *mut GpPen = std::ptr::null_mut();
                            let (color, width) = if is_active {
                                (0xFF00D4FF, 5.0)
                            } else {
                                (0x00, 5.0)
                            };
                            if GdipCreatePen1(color, width, UnitPixel, &mut pen as *mut _ as *mut _) == Status(0) {
                                let x = rect.left as f32;
                                let y = rect.top as f32;
                                let w = (rect.right - rect.left) as f32;
                                let h = (rect.bottom - rect.top) as f32;

                                let mut path: *mut GpPath = std::ptr::null_mut();
                                if GdipCreatePath(FillModeAlternate, &mut path as *mut _ as *mut _) == Status(0) {
                                    let r = 16.0f32;
                                    let x2 = x + w;
                                    let y2 = y + h;

                                    let _ = GdipAddPathArc(path, x2 - 2.0 * r, y, 2.0 * r, 2.0 * r, 270.0, 90.0);
                                    let _ = GdipAddPathLine(path, x2, y + r, x2, y2 - r);
                                    let _ = GdipAddPathArc(path, x2 - 2.0 * r, y2 - 2.0 * r, 2.0 * r, 2.0 * r, 0.0, 90.0);
                                    let _ = GdipAddPathLine(path, x2 - r, y2, x + r, y2);
                                    let _ = GdipAddPathArc(path, x, y2 - 2.0 * r, 2.0 * r, 2.0 * r, 90.0, 90.0);
                                    let _ = GdipAddPathLine(path, x, y2 - r, x, y + r);
                                    let _ = GdipAddPathArc(path, x, y, 2.0 * r, 2.0 * r, 180.0, 90.0);
                                    let _ = GdipAddPathLine(path, x + r, y, x2 - r, y);
                                    let _ = GdipClosePathFigure(path);

                                    let _ = GdipDrawPath(graphics, pen, path);
                                    let _ = GdipDeletePath(path);
                                }
                                let _ = GdipDeletePen(pen);
                            }
                            let _ = GdipDeleteGraphics(graphics);
                        }
                    }
                }

                // Draw text and icons
                let mut text_dc = hdc_buffer;
                let mut hdc_text = HDC::default();
                let mut hbm_text = HBITMAP::default();
                let mut old_text = HGDIOBJ::default();
                let mut text_old_font = HGDIOBJ::default();

                if !s.anim_active {
                    if s.text_anim_active {
                        hdc_text = CreateCompatibleDC(hdc_buffer);
                        hbm_text = CreateCompatibleBitmap(hdc_buffer, s.canvas.screen_w, s.canvas.screen_h);
                        old_text = SelectObject(hdc_text, hbm_text);
                        let _ = BitBlt(hdc_text, 0, 0, s.canvas.screen_w, s.canvas.screen_h, hdc_buffer, 0, 0, SRCCOPY);
                        text_dc = hdc_text;
                        SetBkMode(text_dc, TRANSPARENT);
                        SetTextColor(text_dc, COLORREF(0x00E0E0E0));
                        text_old_font = SelectObject(text_dc, font);
                    }

                    for (idx, cw) in s.canvas.windows.iter().enumerate() {
                        let rect = s.canvas.canvas_to_screen_rect(cw, scale);
                        let icon_size = 20;
                        let icon_spacing = 4;
                        let text_top = rect.bottom + 4;

                        let mut tw: Vec<u16> = cw.title.encode_utf16().collect();
                        let mut measure_rect = RECT {
                            left: 0, top: 0, right: 0, bottom: 0,
                        };
                        let _ = DrawTextW(text_dc, &mut tw, &mut measure_rect, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
                        let text_width = measure_rect.right - measure_rect.left;

                        let total_width = if !cw.icon.is_invalid() {
                            text_width + icon_size + icon_spacing
                        } else {
                            text_width
                        };
                        let start_x = rect.left + (rect.right - rect.left - total_width) / 2;

                        if !cw.icon.is_invalid() {
                            let _ = DrawIconEx(
                                text_dc,
                                start_x, text_top,
                                cw.icon,
                                icon_size, icon_size,
                                0, HBRUSH::default(), DI_NORMAL,
                            );
                        }

                        let text_x = if !cw.icon.is_invalid() {
                            start_x + icon_size + icon_spacing
                        } else {
                            start_x
                        };
                        let mut tr = RECT {
                            left: text_x, top: text_top, right: rect.right, bottom: text_top + 22,
                        };
                        DrawTextW(text_dc, &mut tw, &mut tr, DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                    }

                    if s.text_anim_active {
                        SelectObject(text_dc, text_old_font);
                        let text_t = s.text_anim_step as f64 / TEXT_ANIM_STEPS as f64;
                        let text_alpha = (255.0 * ease_out(text_t)) as u8;
                        let bf = BLENDFUNCTION {
                            BlendOp: AC_SRC_OVER as u8,
                            BlendFlags: 0,
                            SourceConstantAlpha: text_alpha,
                            AlphaFormat: 0,
                        };
                        let _ = AlphaBlend(
                            hdc_buffer, 0, 0, s.canvas.screen_w, s.canvas.screen_h,
                            hdc_text, 0, 0, s.canvas.screen_w, s.canvas.screen_h,
                            bf
                        );
                        SelectObject(hdc_text, old_text);
                        let _ = DeleteObject(hbm_text);
                        let _ = DeleteDC(hdc_text);
                    }
                }

                // Zoom indicator
                let zoom_text = format!("{:.0}%", s.canvas.zoom * 100.0);
                let mut zw: Vec<u16> = zoom_text.encode_utf16().collect();
                let bf_font = CreateFontW(
                    24, 0, 0, 0, 300, 0, 0, 0, 0, 0, 0, 0, 0,
                    PCWSTR(font_name.as_ptr()),
                );
                let of2 = SelectObject(hdc_buffer, bf_font);
                SetTextColor(hdc_buffer, COLORREF(0x00808080));
                let mut zr = RECT {
                    left: s.canvas.screen_w - 120, top: s.canvas.screen_h - 40,
                    right: s.canvas.screen_w - 10, bottom: s.canvas.screen_h - 10,
                };
                DrawTextW(hdc_buffer, &mut zw, &mut zr, DT_RIGHT | DT_SINGLELINE | DT_NOPREFIX);
                SelectObject(hdc_buffer, of2);
                let _ = DeleteObject(bf_font);

                SelectObject(hdc_buffer, old_font);
                let _ = DeleteObject(font);

                // Finally, copy the fully composed buffer to the screen ONCE
                let _ = BitBlt(
                    hdc, 0, 0,
                    s.canvas.screen_w, s.canvas.screen_h,
                    hdc_buffer, 0, 0, SRCCOPY,
                );

                // Clean up buffer
                SelectObject(hdc_buffer, _old_buffer);
                let _ = DeleteObject(hbm_buffer);
                let _ = DeleteDC(hdc_buffer);
            });

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        WM_DESTROY => {
            hotkey::unregister_hotkey(hwnd);
            with_state(|s| {
                if !s.bg_bitmap.0.is_null() {
                    window::free_bitmap(s.bg_bitmap);
                    s.bg_bitmap = HBITMAP::default();
                }
            });
            unsafe {
                let _ = KillTimer(hwnd, TIMER_FADE_IN);
                let _ = KillTimer(hwnd, TIMER_SCROLL_ANIM);
                if GDIPLUS_TOKEN != 0 {
                    let _ = GdiplusShutdown(GDIPLUS_TOKEN);
                    GDIPLUS_TOKEN = 0;
                }
            }
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    // Set up panic hook to log panics
    std::panic::set_hook(Box::new(|info| {
        log_debug(&format!("PANIC: {}", info));
    }));

    log_debug("=== Win-Canvas starting ===");

    // Initialize GDI+
    unsafe {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: false.into(),
            SuppressExternalCodecs: false.into(),
        };
        let mut token = 0usize;
        let result = GdiplusStartup(&mut token, &input, std::ptr::null_mut());
        if result == Status(0) {
            GDIPLUS_TOKEN = token;
            log_debug("GDI+ initialized successfully");
        } else {
            log_debug(&format!("Failed to initialize GDI+: {:?}", result));
        }
    }

    let (screen_w, screen_h) = window::get_screen_size();
    log_debug(&format!("Screen: {}x{}", screen_w, screen_h));

    let mut app_state = AppState::new(screen_w, screen_h);

    let hwnd = match window::create_canvas_window(Some(wndproc)) {
        Ok(h) => {
            log_debug(&format!("Window created: {:?}", h.0));
            h
        }
        Err(e) => {
            log_debug(&format!("Failed to create window: {:?}", e));
            return;
        }
    };
    app_state.canvas_hwnd = hwnd;

    match hotkey::register_hotkey(hwnd) {
        Ok(_) => log_debug("Hotkey Ctrl+Alt+Space registered successfully"),
        Err(e) => {
            log_debug(&format!("Failed to register hotkey: {:?}", e));
            // Try alternative: Ctrl+Alt+Tab
            log_debug("Hotkey registration failed! Another app may have Ctrl+Alt+Space.");
            return;
        }
    }

    APP_STATE.with(|cell| {
        *cell.borrow_mut() = Some(app_state);
    });

    log_debug("Entering message loop...");

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    log_debug("=== Win-Canvas exiting ===");
}
