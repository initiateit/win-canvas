use std::ffi::c_void;
use windows::core::{Interface, Result, ComInterface};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn capture_and_blur_screen(screen_w: i32, screen_h: i32) -> Result<HBITMAP> {
    unsafe {
        // 1. Capture screen with GDI
        let hdc_screen = GetDC(HWND::default());
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm = CreateCompatibleBitmap(hdc_screen, screen_w, screen_h);
        let old = SelectObject(hdc_mem, hbm);
        let _ = BitBlt(hdc_mem, 0, 0, screen_w, screen_h, hdc_screen, 0, 0, SRCCOPY);
        SelectObject(hdc_mem, old);

        // 2. Read pixels
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: screen_w,
                biHeight: -screen_h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixels = vec![0u8; (screen_w * screen_h * 4) as usize];
        GetDIBits(
            hdc_mem,
            hbm,
            0,
            screen_h as u32,
            Some(pixels.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(hdc_mem);
        let _ = DeleteObject(hbm);
        ReleaseDC(HWND::default(), hdc_screen);

        // 3. Initialize D3D11 & D2D
        let mut d3d_device: Option<ID3D11Device> = None;
        let mut d3d_context: Option<ID3D11DeviceContext> = None;
        let hr = D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            Some(&mut d3d_context),
        );
        
        let d3d_device = d3d_device.unwrap();
        let d3d_context = d3d_context.unwrap();
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        
        let mut options = D2D1_FACTORY_OPTIONS::default();
        let factory: ID2D1Factory1 = D2D1CreateFactory(
            D2D1_FACTORY_TYPE_SINGLE_THREADED,
            Some(&options),
        )?;
        
        let d2d_device = factory.CreateDevice(&dxgi_device)?;
        let d2d_context = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

        // 4. Create source D2D bitmap
        let size = D2D_SIZE_U { width: screen_w as u32, height: screen_h as u32 };
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        let src_bitmap = d2d_context.CreateBitmap(
            size,
            Some(pixels.as_ptr() as *const c_void),
            (screen_w * 4) as u32,
            &props,
        )?;

        // 5. Create render target texture (D3D11)
        let tex_desc = D3D11_TEXTURE2D_DESC {
            Width: screen_w as u32,
            Height: screen_h as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        
        let mut rt_tex: Option<ID3D11Texture2D> = None;
        d3d_device.CreateTexture2D(&tex_desc, None, Some(&mut rt_tex))?;
        let rt_tex = rt_tex.unwrap();
        
        let dxgi_surface: IDXGISurface = rt_tex.cast()?;
        
        let target_props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            colorContext: None,
        };
        
        let target_bitmap = d2d_context.CreateBitmapFromDxgiSurface(&dxgi_surface, Some(&target_props))?;
        d2d_context.SetTarget(&target_bitmap);

        // 6. Apply blur
        let blur_effect: ID2D1Effect = d2d_context.CreateEffect(&CLSID_D2D1GaussianBlur)?;
        blur_effect.SetValue(
            D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
            D2D1_PROPERTY_TYPE_FLOAT,
            &120.0f32 as *const _ as *const u8,
            4,
        )?;
        blur_effect.SetValue(
            D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32,
            D2D1_PROPERTY_TYPE_ENUM,
            &D2D1_BORDER_MODE_SOFT.0 as *const _ as *const u8,
            4,
        )?;
        blur_effect.SetInput(0, &src_bitmap, None);

        d2d_context.BeginDraw();
        d2d_context.DrawImage(
            &blur_effect,
            None,
            None,
            D2D1_INTERPOLATION_MODE_LINEAR,
            D2D1_COMPOSITE_MODE_SOURCE_OVER,
        );
        d2d_context.EndDraw(None, None)?;

        // 7. Read back using staging texture
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: screen_w as u32,
            Height: screen_h as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut staging_tex: Option<ID3D11Texture2D> = None;
        d3d_device.CreateTexture2D(&staging_desc, None, Some(&mut staging_tex))?;
        let staging_tex = staging_tex.unwrap();
        
        let rt_resource: ID3D11Resource = rt_tex.cast()?;
        let staging_resource: ID3D11Resource = staging_tex.cast()?;
        d3d_context.CopyResource(&staging_resource, &rt_resource);
        
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        d3d_context.Map(&staging_resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        
        // Create new HBITMAP from mapped data
        let hdc_screen = GetDC(HWND::default());
        let final_hbm = CreateCompatibleBitmap(hdc_screen, screen_w, screen_h);
        SetDIBits(
            hdc_screen,
            final_hbm,
            0,
            screen_h as u32,
            mapped.pData,
            &info,
            DIB_RGB_COLORS,
        );
        
        d3d_context.Unmap(&staging_resource, 0);
        ReleaseDC(HWND::default(), hdc_screen);

        Ok(final_hbm)
    }
}

fn main() {
    let _ = capture_and_blur_screen(1920, 1080);
}
