use windows_sys::Win32::{
    Foundation::RECT,
    Graphics::Gdi::*,
    UI::{Controls::*, WindowsAndMessaging::*},
};

// Palette from the night sky and hydrangeas. Convert RGB notation to COLORREF.
pub const fn rgb(hex: u32) -> u32 {
    hex.swap_bytes() >> 8
}
pub const PANEL: u32 = rgb(0x151d36);
pub const FIELD: u32 = rgb(0x222e49);

pub unsafe fn rounded(dc: HDC, r: RECT, radius: i32, fill: u32, edge: u32) {
    unsafe {
        let brush = SelectObject(dc, GetStockObject(DC_BRUSH));
        let pen = SelectObject(dc, GetStockObject(DC_PEN));
        SetDCBrushColor(dc, fill);
        SetDCPenColor(dc, edge);
        RoundRect(dc, r.left, r.top, r.right, r.bottom, radius * 2, radius * 2);
        SelectObject(dc, pen);
        SelectObject(dc, brush);
    }
}

pub unsafe fn draw_button(draw: &NMCUSTOMDRAW, primary: bool) {
    let dc = draw.hdc;
    let state = draw.uItemState;
    let disabled = state & (CDIS_DISABLED | CDIS_GRAYED) != 0
        || unsafe { GetWindowLongW(draw.hdr.hwndFrom, GWL_STYLE) as u32 & WS_DISABLED != 0 };
    let pressed = state & CDIS_SELECTED != 0;
    let hot = state & CDIS_HOT != 0;
    let (fill, ink, edge) = if disabled {
        (rgb(0x212b42), rgb(0x929db4), rgb(0x303b54))
    } else if primary {
        (
            if pressed {
                rgb(0x9baff1)
            } else if hot {
                rgb(0xd6e1ff)
            } else {
                rgb(0xbacbff)
            },
            rgb(0x152547),
            rgb(0xd4dfff),
        )
    } else {
        (
            if pressed {
                rgb(0x283959)
            } else if hot {
                rgb(0x384b70)
            } else {
                FIELD
            },
            rgb(0xebefff),
            rgb(0x495976),
        )
    };
    unsafe {
        let saved = SaveDC(dc);
        SetDCBrushColor(dc, PANEL);
        FillRect(dc, &draw.rc, GetStockObject(DC_BRUSH));
        rounded(
            dc,
            draw.rc,
            9,
            fill,
            if state & CDIS_FOCUS != 0 {
                rgb(0xffffff)
            } else {
                edge
            },
        );
        let hwnd = draw.hdr.hwndFrom;
        let mut title = [0u16; 128];
        let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        let font = SendMessageW(hwnd, WM_GETFONT, 0, 0);
        if font != 0 {
            SelectObject(dc, font as _);
        }
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, ink);
        let mut r = draw.rc;
        let cues = if state & CDIS_SHOWKEYBOARDCUES == 0 {
            DT_HIDEPREFIX
        } else {
            0
        };
        DrawTextW(
            dc,
            title.as_ptr(),
            len,
            &mut r,
            DT_SINGLELINE | DT_CENTER | DT_VCENTER | cues,
        );
        RestoreDC(dc, saved);
    }
}
