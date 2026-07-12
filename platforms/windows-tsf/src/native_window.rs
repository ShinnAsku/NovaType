#![cfg(windows)]

use crate::candidate_window::CandidateWindowView;
use crate::window::{
    CandidateWindowHandle, CandidateWindowMetrics, PaintColor, PaintRenderer, Rect,
    WINDOW_CLASS_NAME, render_commands,
};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
    EndPaint, FillRect, HBRUSH, HDC, PAINTSTRUCT, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    GWLP_USERDATA, HMENU, HWND_TOPMOST, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_NCDESTROY, WM_PAINT, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::PCWSTR;

/// RAII wrapper for the native candidate popup HWND.
#[derive(Debug)]
pub struct NativeCandidateWindow {
    hwnd: HWND,
    state: Box<NativeCandidateWindowState>,
}

#[derive(Debug)]
struct NativeCandidateWindowState {
    view: Option<CandidateWindowView>,
    metrics: CandidateWindowMetrics,
}

/// Minimal GDI renderer for candidate-window paint commands.
pub struct GdiCommandRenderer {
    hdc: HDC,
}

impl GdiCommandRenderer {
    #[must_use]
    pub fn new(hdc: HDC) -> Self {
        Self { hdc }
    }
}

impl PaintRenderer for GdiCommandRenderer {
    fn fill_rect(&mut self, rect: Rect, color: PaintColor) {
        let brush = unsafe { CreateSolidBrush(color_ref(color)) };
        let native = to_rect(rect);
        unsafe {
            let _ = FillRect(self.hdc, &raw const native, brush);
            let _ = DeleteObject(brush);
        }
    }

    fn stroke_rect(&mut self, rect: Rect, color: PaintColor) {
        // Placeholder: draw a one-pixel border with four filled rectangles.
        let edges = [
            Rect { height: 1, ..rect },
            Rect {
                y: rect.y + rect.height - 1,
                height: 1,
                ..rect
            },
            Rect { width: 1, ..rect },
            Rect {
                x: rect.x + rect.width - 1,
                width: 1,
                ..rect
            },
        ];
        for edge in edges {
            self.fill_rect(edge, color);
        }
    }

    fn text(&mut self, rect: Rect, text: &str, color: PaintColor) {
        let mut native = to_rect(rect);
        let mut wide = wide(text);
        let len = wide.len().saturating_sub(1);
        unsafe {
            let _ = SetTextColor(self.hdc, color_ref(color));
            let _ = SetBkMode(self.hdc, TRANSPARENT);
            let _ = DrawTextW(
                self.hdc,
                &mut wide[..len],
                &raw mut native,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }
}

fn color_ref(color: PaintColor) -> windows::Win32::Foundation::COLORREF {
    let rgb = match color {
        PaintColor::Background => 0x00FF_FFFF,
        PaintColor::Border => 0x00DD_D6CC,
        PaintColor::Brand => 0x0098_7B19,
        PaintColor::Text => 0x002E_261B,
        PaintColor::MutedText => 0x0090_836C,
        PaintColor::HighlightBackground => 0x00F3_E6D4,
    };
    windows::Win32::Foundation::COLORREF(rgb)
}

fn to_rect(rect: Rect) -> RECT {
    RECT {
        left: rect.x,
        top: rect.y,
        right: rect.x + rect.width,
        bottom: rect.y + rect.height,
    }
}

impl NativeCandidateWindow {
    /// Creates a hidden candidate popup window.
    ///
    /// # Errors
    ///
    /// Returns an error if the class cannot be registered or the window cannot
    /// be created.
    pub fn create() -> windows::core::Result<Self> {
        let class_name = wide(WINDOW_CLASS_NAME);
        let instance = unsafe { GetModuleHandleW(None)? };
        register_class(instance.into(), &class_name);

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WS_POPUP,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1,
                1,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )
        }?;

        let mut state = Box::new(NativeCandidateWindowState {
            view: None,
            metrics: CandidateWindowMetrics::default(),
        });
        unsafe {
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&raw mut *state) as isize);
        }
        Ok(Self { hwnd, state })
    }

    #[must_use]
    pub fn handle(&self) -> CandidateWindowHandle {
        CandidateWindowHandle(self.hwnd.0 as isize)
    }

    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// # Errors
    ///
    /// Returns an error when Windows rejects the move/resize operation.
    pub fn update_bounds(&self, rect: Rect) -> windows::core::Result<()> {
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        }
    }

    /// # Errors
    ///
    /// Returns an error when Windows rejects the move/resize operation.
    pub fn update_view(
        &mut self,
        view: &CandidateWindowView,
        caret: (i32, i32),
        metrics: CandidateWindowMetrics,
    ) -> windows::core::Result<()> {
        self.state.metrics = metrics;
        self.state.view = Some(view.clone());
        self.update_bounds(metrics.position_near_caret(caret, view))?;
        self.show();
        Ok(())
    }
}

impl Drop for NativeCandidateWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn register_class(instance: HINSTANCE, class_name: &[u16]) {
    let wnd_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hbrBackground: HBRUSH::default(),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..WNDCLASSW::default()
    };

    let atom = unsafe { RegisterClassW(&raw const wnd_class) };
    if atom == 0 {
        // RegisterClassW returns 0 when the class already exists too; in that
        // case creating the window still works, so do not fail here.
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_PAINT {
        unsafe {
            paint_window(hwnd);
        }
        return LRESULT(0);
    }
    if msg == WM_NCDESTROY {
        unsafe {
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn paint_window(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &raw mut ps) };
    if !hdc.is_invalid() {
        let ptr = unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
        } as *mut NativeCandidateWindowState;
        if let Some(state) = unsafe { ptr.as_ref() }
            && let Some(view) = &state.view
        {
            let commands = state.metrics.paint_commands(view);
            let mut renderer = GdiCommandRenderer::new(hdc);
            render_commands(&mut renderer, &commands);
        }
    }
    unsafe {
        let _ = EndPaint(hwnd, &raw const ps);
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::{color_ref, wide};
    use crate::window::PaintColor;

    #[test]
    fn wide_strings_are_null_terminated() {
        let wide = wide("NovaTypeCandidateWindow");

        assert_eq!(wide.last(), Some(&0));
    }

    #[test]
    fn maps_brand_color() {
        assert_ne!(
            color_ref(PaintColor::Brand),
            color_ref(PaintColor::Background)
        );
    }
}
