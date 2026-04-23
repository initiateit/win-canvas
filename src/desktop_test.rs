use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

fn wide_string(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    unsafe {
        let progman = FindWindowW(PCWSTR(wide_string("Progman").as_ptr()), None);
        println!("Progman: {:?}", progman);
        let hdc = GetDC(progman);
        println!("HDC: {:?}", hdc);
    }
}
