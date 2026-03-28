//! DWM Thumbnail management — register, update, and unregister live window thumbnails.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_VISIBLE, DWM_TNP_RECTDESTINATION,
    DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_OPACITY,
};

/// A managed DWM thumbnail.
pub struct Thumbnail {
    pub handle: isize,
    pub source_hwnd: HWND,
    pub source_width: i32,
    pub source_height: i32,
}

impl Thumbnail {
    /// Register a DWM thumbnail from `source` onto `destination` window.
    pub fn register(destination: HWND, source: HWND) -> windows::core::Result<Self> {
        unsafe {
            let handle = DwmRegisterThumbnail(destination, source)?;
            let source_size = DwmQueryThumbnailSourceSize(handle)?;

            Ok(Self {
                handle,
                source_hwnd: source,
                source_width: source_size.cx,
                source_height: source_size.cy,
            })
        }
    }

    /// Update the thumbnail display properties (position/size on the destination window).
    /// The rect is inset to create negative space for rounded corners.
    pub fn update(&self, dest_rect: RECT, opacity: u8, client_area_only: bool) -> windows::core::Result<()> {
        unsafe {
            // Inset by 16px (corner radius) to create negative space for rounded corners
            // The background shows through at the corners
            let inset_rect = RECT {
                left: dest_rect.left + 16,
                top: dest_rect.top + 16,
                right: dest_rect.right - 16,
                bottom: dest_rect.bottom - 16,
            };

            let mut props = DWM_THUMBNAIL_PROPERTIES::default();
            props.dwFlags = DWM_TNP_VISIBLE | DWM_TNP_RECTDESTINATION | DWM_TNP_OPACITY;
            props.fVisible = true.into();
            props.rcDestination = inset_rect;
            props.opacity = opacity;

            if client_area_only {
                props.dwFlags |= DWM_TNP_SOURCECLIENTAREAONLY;
                props.fSourceClientAreaOnly = true.into();
            }

            DwmUpdateThumbnailProperties(self.handle, &props)?;
            Ok(())
        }
    }

    /// Hide this thumbnail (set invisible).
    pub fn hide(&self) -> windows::core::Result<()> {
        unsafe {
            let mut props = DWM_THUMBNAIL_PROPERTIES::default();
            props.dwFlags = DWM_TNP_VISIBLE;
            props.fVisible = false.into();
            DwmUpdateThumbnailProperties(self.handle, &props)?;
            Ok(())
        }
    }

    /// Get the aspect ratio (width / height).
    pub fn aspect_ratio(&self) -> f64 {
        if self.source_height == 0 {
            1.0
        } else {
            self.source_width as f64 / self.source_height as f64
        }
    }
}

impl Drop for Thumbnail {
    fn drop(&mut self) {
        unsafe {
            let _ = DwmUnregisterThumbnail(self.handle);
        }
    }
}
