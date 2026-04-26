# Changelog — Cleanup: Fix Warnings & Remove Unused Dependencies

## Branch: `cleanup/fix-warnings-and-unused-deps`

---

## 1. Compilation Warnings Fixed

### `src/window.rs:222` — Unnecessary `mut` qualifier
**Before:**
```rust
let mut target_props = D2D1_BITMAP_PROPERTIES1 { ... };
```
**After:**
```rust
let target_props = D2D1_BITMAP_PROPERTIES1 { ... };
```
The variable was never mutated after initialization, so `mut` was removed.

---

### `src/main.rs:664` — Unused variable `idx` in text-drawing loop
**Before:**
```rust
for (idx, cw) in s.canvas.windows.iter().enumerate() {
```
**After:**
```rust
for cw in s.canvas.windows.iter() {
```
The index was never used inside the loop body, so `.enumerate()` was removed.

---

### `src/canvas.rs:249` — Dead method `is_scrolling()`
Removed the entire method. It was a public getter for `self.scroll_active` but was never called anywhere in the codebase.

---

### `src/canvas.rs:303` — Dead method `start_drag()`
Removed the entire method. Window dragging was disabled (only right-click canvas panning is active), so this method was unreachable.

---

### `src/dwm.rs:66` — Dead method `aspect_ratio()`
Removed the entire method. It computed `width / height` for a thumbnail but was never called.

---

### `src/input.rs:5` — Unused enum `MouseEvent`
Removed the entire enum. Mouse events are handled inline in the `wndproc` using raw `LPARAM`/`WPARAM` parsing via `mouse_coords()` and `wheel_delta()`, so the enum was never constructed or matched.

---

### `src/main.rs` — Mutable references to `static mut` (Rust 2024 compat)
**Before:**
```rust
static mut GDIPLUS_TOKEN: usize = 0;
static mut SHADOW_IMAGE: *mut GpImage = std::ptr::null_mut();

// ... later:
GDIPLUS_TOKEN = token;
&mut SHADOW_IMAGE
SHADOW_IMAGE.is_null()
```
**After:**
```rust
use std::ptr::{addr_of, addr_of_mut};

// ... later:
*addr_of_mut!(GDIPLUS_TOKEN) = token;
addr_of_mut!(SHADOW_IMAGE)
(*addr_of!(SHADOW_IMAGE)).is_null()
```
Rust 2024 edition deprecates creating `&mut` references to `static mut` (UB risk). All accesses were replaced with `addr_of_mut!` (for writes) and `addr_of!` (for reads).

---

## 2. Unused Cargo Features Removed

The following `windows` crate features were enabled in `Cargo.toml` but had no corresponding `use` statements in any source file:

| Feature | Reason |
|---|---|
| `Win32_UI_HiDpi` | No imports from `Win32::UI::HiDpi` |
| `Win32_UI_Shell` | No imports from `Win32::UI::Shell` |
| `Win32_System_Threading` | No imports from `Win32::System::Threading` |
| `Win32_System_Com` | No imports from `Win32::System::Com` (COM types used via `windows::core`) |
| `Win32_Graphics_DirectWrite` | No imports from `Win32::Graphics::DirectWrite` (text rendering done via GDI+) |

---

## 3. Dead Test Files Removed

These files each contained their own `fn main()` and were never referenced from the project's `main.rs` or configured as separate binary targets. They were standalone experiments:

| File | Purpose |
|---|---|
| `src/d2d_test.rs` | Direct2D screen capture + blur experiment |
| `src/desktop_test.rs` | Progman window HDC test |
| `src/gdi_test.rs` | GDI+ font family loading test |

---

## Result

**Before:** 6 compiler warnings
**After:** 0 warnings, 0 errors — clean build
