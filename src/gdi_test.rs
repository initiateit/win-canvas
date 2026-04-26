use windows::Win32::Graphics::GdiPlus::*;
use windows::core::PCWSTR;

fn main() {
    unsafe {
        let mut token: usize = 0;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: None,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        GdiplusStartup(&mut token, &input, std::ptr::null_mut());

        let family_name = "Segoe UI Semibold".encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let mut font_family: *mut GpFontFamily = std::ptr::null_mut();
        let status = GdipCreateFontFamilyFromName(PCWSTR(family_name.as_ptr()), std::ptr::null_mut(), &mut font_family);
        
        println!("Status for 'Segoe UI Semibold': {:?}", status);

        let family_name2 = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let mut font_family2: *mut GpFontFamily = std::ptr::null_mut();
        let status2 = GdipCreateFontFamilyFromName(PCWSTR(family_name2.as_ptr()), std::ptr::null_mut(), &mut font_family2);
        
        println!("Status for 'Segoe UI': {:?}", status2);
    }
}
