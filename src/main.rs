#![windows_subsystem = "windows"]
mod assets;
mod install;
mod theme;
use windows_sys::Win32::{Foundation::RECT, UI::Controls::*};

use anyhow::{Context, Result};
use native_windows_gui as nwg;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::*};

enum Update {
    Initial(Result<PathBuf>),
    Progress(u32, String),
    Finished(Result<(PathBuf, String, bool)>),
}

fn label(
    window: &nwg::Window,
    text: &str,
    pos: (i32, i32),
    size: (i32, i32),
    font: Option<&nwg::Font>,
) -> Result<nwg::Label> {
    let mut label = Default::default();
    nwg::Label::builder()
        .parent(window)
        .text(text)
        .position(pos)
        .size(size)
        .font(font)
        .build(&mut label)?;
    Ok(label)
}

fn button(
    window: &nwg::Window,
    text: &str,
    pos: (i32, i32),
    size: (i32, i32),
) -> Result<nwg::Button> {
    let mut button = Default::default();
    nwg::Button::builder()
        .parent(window)
        .text(text)
        .position(pos)
        .size(size)
        .build(&mut button)?;
    Ok(button)
}

fn pick(window: &nwg::Window, folder: bool, start: &Path) -> Result<Option<PathBuf>> {
    let mut dialog = Default::default();
    let builder = nwg::FileDialog::builder();
    let builder = if folder {
        builder
            .title("Choisis un disque ou un dossier de jeux")
            .action(nwg::FileDialogAction::OpenDirectory)
    } else {
        builder
            .title("Choisis ton ISO de Mahoyo PC 2012")
            .filters("Image disque (*.iso)")
            .action(nwg::FileDialogAction::Open)
    };
    builder.build(&mut dialog)?;
    if let Some(p) = start
        .ancestors()
        .find(|p| p.is_dir())
        .and_then(Path::to_str)
    {
        dialog.set_default_folder(p)?;
    }
    if dialog.run(Some(window)) {
        Ok(Some(dialog.get_selected_item()?.into()))
    } else {
        Ok(None)
    }
}

fn run() -> Result<()> {
    if !install::request_admin()? {
        return Ok(());
    }
    nwg::init()?;
    nwg::Font::set_global_family("Segoe UI")?;
    let base = std::env::current_exe()?
        .parent()
        .context("Dossier du launcher introuvable.")?
        .to_owned();
    let default_parent = base.ancestors().last().unwrap().join("Jeux");
    let icon = nwg::EmbedResource::load(None)?
        .icon(1, Some((32, 32)))
        .context("Icône introuvable")?;
    let mut window = Default::default();
    nwg::Window::builder()
        .title("Installateur Mahoyo")
        .size((1000, 563))
        .center(true)
        .icon(Some(&icon))
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::MINIMIZE_BOX)
        .build(&mut window)?;
    let window = Rc::new(window);
    let bitmap = nwg::Bitmap::from_bin(include_bytes!("../sources/background.png"))?;
    let mut heading = Default::default();
    nwg::Font::builder()
        .family("Georgia")
        .size(25)
        .build(&mut heading)?;
    let mut body = nwg::Font::default();
    nwg::Font::builder()
        .family("Segoe UI")
        .size(18)
        .build(&mut body)?;
    let _title = label(
        &window,
        "Français et voix japonaises",
        (410, 329),
        (354, 34),
        Some(&heading),
    )?;
    let _edition = label(
        &window,
        "Édition PC 2012   ·   Environ 11 Go à télécharger",
        (410, 365),
        (530, 20),
        Some(&body),
    )?;
    let _prompt = label(
        &window,
        "Dossier d’installation",
        (410, 397),
        (530, 20),
        Some(&body),
    )?;
    let iso = button(&window, "Choisir une ISO &locale", (770, 331), (180, 34))?;
    let mut destination = Default::default();
    nwg::TextInput::builder()
        .parent(&*window)
        .position((421, 427))
        .size((383, 23))
        .font(Some(&body))
        .background_color(Some([0x22, 0x2e, 0x49]))
        .readonly(true)
        .build(&mut destination)?;
    let browse = button(&window, "&Modifier…", (828, 417), (122, 38))?;
    let mut bar = Default::default();
    nwg::ProgressBar::builder()
        .parent(&*window)
        .position((410, 468))
        .size((540, 4))
        .range(0..100)
        .build(&mut bar)?;
    let status = label(
        &window,
        "Préparation du dossier…",
        (410, 483),
        (261, 56),
        Some(&body),
    )?;
    let install_btn = button(&window, "&Installer", (682, 487), (124, 40))?;
    let launch = button(&window, "&Lancer le jeu", (816, 487), (134, 40))?;
    for btn in [&iso, &browse, &install_btn, &launch] {
        btn.set_font(Some(&body));
    }
    bar.set_visible(false);
    let buttons = [&iso, &browse, &install_btn, &launch].map(|b| b.handle.hwnd().unwrap().cast());
    let field = destination.handle.hwnd().unwrap().cast();
    unsafe {
        let style = GetWindowLongW(field, GWL_STYLE);
        SetWindowLongW(field, GWL_STYLE, style & !(WS_BORDER as i32));
        let ex_style = GetWindowLongW(field, GWL_EXSTYLE);
        SetWindowLongW(field, GWL_EXSTYLE, ex_style & !(WS_EX_CLIENTEDGE as i32));
        SetWindowPos(
            field,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOZORDER | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    launch.set_enabled(false);
    install_btn.set_enabled(false);
    browse.set_enabled(false);
    let paint = nwg::bind_raw_event_handler(&window.handle, 0x10001, move |_, msg, w, l| {
        if msg == WM_NOTIFY && l != 0 {
            let hdr = unsafe { &*(l as *const NMHDR) };
            if hdr.code == NM_CUSTOMDRAW && buttons.contains(&hdr.hwndFrom) {
                let draw = unsafe { &*(l as *const NMCUSTOMDRAW) };
                if draw.dwDrawStage == CDDS_PREPAINT {
                    unsafe {
                        theme::draw_button(draw, hdr.hwndFrom == buttons[2]);
                    }
                    return Some(CDRF_SKIPDEFAULT as isize);
                }
            }
        }
        if msg == WM_ERASEBKGND {
            let dc = w as _;
            unsafe {
                let memory = CreateCompatibleDC(dc);
                let previous = SelectObject(memory, bitmap.handle.cast());
                BitBlt(dc, 0, 0, 1000, 563, memory, 0, 0, SRCCOPY);
                SelectObject(memory, previous);
                DeleteDC(memory);
                theme::rounded(
                    dc,
                    RECT {
                        left: 386,
                        top: 310,
                        right: 976,
                        bottom: 547,
                    },
                    17,
                    theme::PANEL,
                    theme::rgb(0x526480),
                );
                theme::rounded(
                    dc,
                    RECT {
                        left: 410,
                        top: 417,
                        right: 816,
                        bottom: 455,
                    },
                    9,
                    theme::FIELD,
                    theme::rgb(0x394964),
                );
            }
            Some(1)
        } else if msg == WM_CTLCOLORSTATIC {
            let dc = w as _;
            unsafe {
                SetTextColor(dc, 0x00fffaff);
                let fill = if l as usize == field as usize {
                    theme::FIELD
                } else {
                    theme::PANEL
                };
                SetBkColor(dc, fill);
                SetDCBrushColor(dc, fill);
                Some(GetStockObject(DC_BRUSH) as isize)
            }
        } else {
            None
        }
    })?;
    let mut notice = Default::default();
    nwg::Notice::builder().parent(&*window).build(&mut notice)?;
    let (tx, rx) = mpsc::channel();
    let local = RefCell::new(None::<PathBuf>);
    let installed = RefCell::new(None::<PathBuf>);
    let busy = Cell::new(false);
    let initial = notice.sender();
    let init_tx = tx.clone();
    window.set_visible(true);
    thread::spawn(move || {
        let _ = init_tx.send(Update::Initial(install::suggest(&default_parent)));
        initial.notice();
    });
    let owner = window.clone();
    let handler = nwg::full_bind_event_handler(&window.handle, move |event, data, handle| {
        let error = |e: anyhow::Error| {
            nwg::modal_error_message(&*owner, "Installateur Mahoyo", &format!("{e:#}"));
        };
        match event {
            nwg::Event::OnWindowClose if handle == owner.handle => {
                if busy.get() {
                    if let nwg::EventData::OnWindowClose(d) = data {
                        d.close(false);
                    }
                    nwg::modal_info_message(
                        &*owner,
                        "Installateur Mahoyo",
                        "Attends la fin de l’installation avant de fermer.",
                    );
                } else {
                    nwg::stop_thread_dispatch();
                }
            }
            nwg::Event::OnButtonClick if handle == browse.handle || handle == iso.handle => {
                let folder = handle == browse.handle;
                let path = PathBuf::from(destination.text());
                let result =
                    pick(&owner, folder, path.parent().unwrap_or(&base)).and_then(|choice| {
                        if let Some(p) = choice {
                            if folder {
                                destination.set_text(&install::suggest(&p)?.to_string_lossy());
                                install_btn.set_enabled(true);
                            } else {
                                local.replace(Some(p));
                                iso.set_text("ISO locale sélectionnée");
                            }
                        }
                        Ok(())
                    });
                if let Err(e) = result {
                    error(e);
                }
            }
            nwg::Event::OnButtonClick if handle == install_btn.handle && !busy.replace(true) => {
                let configure = !install::japanese()
                    && nwg::modal_message(
                        &*owner,
                        &nwg::MessageParams {
                            title: "Installateur Mahoyo",
                            content: "Configurer le japonais pour les programmes non Unicode et désactiver UTF-8 bêta ? Ce réglage concerne tous les anciens programmes de ce PC. Un redémarrage manuel sera nécessaire pour jouer.",
                            buttons: nwg::MessageButtons::YesNo,
                            icons: nwg::MessageIcons::Question,
                        },
                    ) == nwg::MessageChoice::Yes;
                for btn in [&install_btn, &browse, &iso, &launch] {
                    btn.set_enabled(false);
                }
                let target = PathBuf::from(destination.text());
                let base = base.clone();
                let local = local.borrow().clone();
                let sender = notice.sender();
                let tx = tx.clone();
                bar.set_visible(true);
                bar.set_pos(0);
                status.set_text("Vérification des données…");
                thread::spawn(move || {
                    let last = Cell::new(None::<Instant>);
                    let report = |n, msg: &str| {
                        if last
                            .get()
                            .is_none_or(|t| t.elapsed() >= Duration::from_millis(150))
                        {
                            last.set(Some(Instant::now()));
                            let _ = tx.send(Update::Progress(n, msg.into()));
                            sender.notice();
                        }
                    };
                    let result = (|| {
                        if configure {
                            report(0, "Configuration du japonais…");
                            install::configure_japanese()?;
                        }
                        install::install(&base, &target, local.as_deref(), &report)
                    })()
                    .map(|mut msg| {
                        if configure {
                            msg.push_str(" Redémarre Windows avant de jouer.");
                        }
                        match install::shortcut(&target) {
                            Ok(()) => msg.push_str(" Raccourci créé sur le Bureau."),
                            Err(e) => msg.push_str(&format!(" Raccourci non créé : {e}")),
                        };
                        (target, msg, configure)
                    });
                    let _ = tx.send(Update::Finished(result));
                    sender.notice();
                });
            }
            nwg::Event::OnButtonClick if handle == launch.handle => {
                if let Some(target) = installed.borrow().as_ref()
                    && let Err(e) = install::launch(target)
                {
                    error(e);
                }
            }
            nwg::Event::OnNotice if handle == notice.handle => {
                for update in rx.try_iter() {
                    match update {
                        Update::Initial(result) => {
                            browse.set_enabled(true);
                            match result {
                                Ok(p) => {
                                    destination.set_text(&p.to_string_lossy());
                                    install_btn.set_enabled(true);
                                    status.set_text(if install::japanese(){"Prêt à installer."}else{"Pour jouer, sélectionne le japonais pour les programmes non Unicode et désactive UTF-8 bêta."});
                                }
                                Err(e) => {
                                    status.set_text("Choisis un dossier avec Modifier…");
                                    error(e);
                                }
                            }
                        }
                        Update::Progress(n, msg) => {
                            bar.set_pos(n.min(100));
                            status.set_text(&msg);
                        }
                        Update::Finished(result) => {
                            busy.set(false);
                            for btn in [&install_btn, &browse, &iso] {
                                btn.set_enabled(true);
                            }
                            match result {
                                Ok((target, msg, restart)) => {
                                    installed.replace(Some(target));
                                    bar.set_pos(100);
                                    status.set_text(&msg);
                                    if restart {
                                        launch.set_text("Redémarrage requis");
                                        nwg::modal_info_message(&*owner,"Installation terminée","Le jeu est installé et les paramètres japonais sont enregistrés.

Redémarre Windows avant de lancer le jeu.");
                                    }
                                }
                                Err(e) => {
                                    status.set_text("Installation interrompue.");
                                    error(e);
                                }
                            }
                            launch.set_enabled(installed.borrow().is_some() && install::japanese());
                        }
                    }
                }
            }
            _ => (),
        }
    });
    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
    nwg::unbind_raw_event_handler(&paint)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        nwg::error_message("Installateur Mahoyo", &format!("{e:#}"));
    }
}
