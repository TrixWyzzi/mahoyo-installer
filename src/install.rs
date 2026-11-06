use crate::assets::{self, Asset, GAME, Report, no_links};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

pub fn suggest(parent: &Path) -> Result<PathBuf> {
    let name = "Mahoyo-FR-Voix";
    let first = if parent
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
    {
        parent.to_owned()
    } else {
        parent.join(name)
    };
    let mut path = first.clone();
    let mut i = 2;
    while path.try_exists()? {
        let mut n = first.as_os_str().to_owned();
        n.push(format!(" ({i})"));
        path = PathBuf::from(n);
        i += 1;
    }
    Ok(path)
}

fn overlaps(a: &Path, b: &Path) -> bool {
    let a = PathBuf::from(a.to_string_lossy().to_lowercase());
    let b = PathBuf::from(b.to_string_lossy().to_lowercase());
    a.starts_with(&b) || b.starts_with(&a)
}

fn validate(target: &Path, base: &Path) -> Result<()> {
    ensure!(
        target.is_absolute()
            && target.parent().is_some()
            && !target
                .components()
                .any(|c| matches!(c, Component::ParentDir)),
        "Dossier d’installation invalide."
    );
    ensure!(
        !overlaps(target, base)
            && !overlaps(target, &target.parent().unwrap().join(".mahoyo-cache")),
        "Choisis un dossier distinct de l’installateur et du cache."
    );
    no_links(target)?;
    ensure!(
        !target.try_exists()? || target.is_dir() && target.read_dir()?.next().is_none(),
        "Le dossier est occupé. Choisis un nouveau dossier pour préserver les sauvegardes."
    );
    Ok(())
}

fn powershell(script: &str, path: &Path) -> Result<String> {
    let exe = PathBuf::from(std::env::var_os("SystemRoot").context("Windows introuvable")?)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let mut child=Command::new(exe).args(["-NoProfile","-NonInteractive","-Command"])
        .arg(format!("$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$p=$env:MAHOYO_PATH;{script}"))
        .env("MAHOYO_PATH",path).creation_flags(0x08000000).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    if child.wait_timeout(Duration::from_secs(120))?.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        bail!("Windows ne répond pas. Vérifie le lecteur virtuel dans l’Explorateur.");
    }
    let out = child.wait_with_output()?;
    ensure!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

struct Disc<'a> {
    path: &'a Path,
    owned: bool,
    source: PathBuf,
}
impl<'a> Disc<'a> {
    fn open(path: &'a Path) -> Result<Self> {
        let response = powershell(
            r#"$own=$false;try{$d=Get-DiskImage -ImagePath $p;if(!$d.Attached){Mount-DiskImage -ImagePath $p|Out-Null;$own=$true};$v=Get-DiskImage -ImagePath $p|Get-Volume;if(!$v.DriveLetter){throw 'Lecteur introuvable'};Write-Output ($own.ToString()+'|'+$v.DriveLetter)}catch{if($own){Dismount-DiskImage -ImagePath $p -ErrorAction SilentlyContinue|Out-Null};throw}"#,
            path,
        )?;
        let (own, letter) = response
            .split_once('|')
            .context("Réponse inattendue du lecteur virtuel.")?;
        ensure!(
            matches!(own, "True" | "False")
                && letter.len() == 1
                && letter.as_bytes()[0].is_ascii_alphabetic(),
            "Lecteur virtuel invalide."
        );
        let disc = Self {
            path,
            owned: own == "True",
            source: PathBuf::from(format!("{letter}:\\{GAME}")),
        };
        ensure!(disc.source.is_dir(), "Le disque ne contient pas le jeu.");
        Ok(disc)
    }
    fn close(&mut self) -> Result<()> {
        if self.owned {
            powershell("Dismount-DiskImage -ImagePath $p|Out-Null", self.path)?;
            self.owned = false;
        }
        Ok(())
    }
}
impl Drop for Disc<'_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn copy_game(
    source: &Path,
    target: &Path,
    done: &mut u64,
    buffer: &mut [u8],
    report: &Report,
) -> Result<()> {
    for entry in source.read_dir()? {
        let e = entry?;
        let path = e.path();
        no_links(&path)?;
        let dest = target.join(e.file_name());
        if e.file_type()?.is_dir() {
            fs::create_dir(&dest)?;
            copy_game(&path, &dest, done, buffer, report)?;
        } else {
            let mut input = File::open(path)?;
            let mut out = File::create_new(&dest)?;
            let message = format!("Copie du jeu · {}", e.file_name().to_string_lossy());
            loop {
                let n = input.read(buffer)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buffer[..n])?;
                *done += n as u64;
                report((20 + (*done * 60 / 7308540406).min(60)) as u32, &message);
            }
        }
    }
    Ok(())
}

fn stage(
    base: &Path,
    target: &Path,
    source: &Path,
    assets: &[Asset],
    report: &Report,
) -> Result<()> {
    validate(target, base)?;
    ensure!(
        !overlaps(source, target),
        "Source et destination identiques."
    );
    let parent = target.parent().unwrap();
    let stage = tempfile::Builder::new()
        .prefix(".mahoyo-installation-")
        .tempdir_in(parent)?;
    let mut buf = vec![0; 1024 * 1024];
    copy_game(source, stage.path(), &mut 0, &mut buf, report)?;
    let patches = &assets[1..];
    let total: u64 = patches.iter().map(|a| a.len).sum();
    let mut done = 0u64;

    for a in patches {
        let relative = if a.name.starts_with("patches\\") {
            Path::new("savedata").join(&a.name)
        } else {
            PathBuf::from(
                a.name
                    .strip_prefix("correctifs\\")
                    .context("Correctif inconnu")?,
            )
        };
        let dest = stage.path().join(relative);
        fs::create_dir_all(dest.parent().unwrap())?;
        let mut input = File::open(&a.path)?;
        let mut output = File::create(dest)?;
        let mut hash = Sha256::new();
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
            done += n as u64;
            report(
                (80 + done * 19 / total) as u32,
                &format!("Installation · {}", a.name),
            );
        }
        ensure!(
            format!("{:X}", hash.finalize()) == a.hash,
            "Données modifiées : {}",
            a.name
        );
    }
    ensure!(
        stage.path().join(format!("{GAME}.exe")).is_file(),
        "Exécutable du jeu absent."
    );
    for attempt in 0..10 {
        validate(target, base)?;
        let promote = || -> std::io::Result<()> {
            if target.try_exists()? {
                fs::remove_dir(target)?;
            }
            fs::rename(stage.path(), target)
        };
        match promote() {
            Ok(()) => {
                let _ = stage.keep();
                return Ok(());
            }
            Err(e) if attempt < 9 && matches!(e.raw_os_error(), Some(5 | 32 | 33)) => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!()
}

pub fn install(
    base: &Path,
    target: &Path,
    local: Option<&Path>,
    report: &Report,
) -> Result<String> {
    validate(target, base)?;
    let cache = target
        .parent()
        .unwrap()
        .join(".mahoyo-cache")
        .join("donnees");
    let lock_path = cache.join("installation.lock");
    no_links(&lock_path)?;
    fs::create_dir_all(&cache)?;
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.try_lock()
        .context("Une autre installation utilise déjà ce cache.")?;
    let assets = assets::prepare(base, &cache, local, report)?;
    report(20, "Montage du disque du jeu…");
    let mut disc = Disc::open(&assets[0].path)?;
    assets::disc_corrections(&assets, &disc.source)?;
    stage(base, target, &disc.source, &assets, report)?;
    let mut message = String::from("Installation terminée.");
    if let Err(e) = disc.close() {
        message.push_str(&format!(" Démontage manuel nécessaire : {e}"));
    }
    Ok(message)
}

pub fn shortcut(target: &Path) -> Result<()> {
    powershell(
        r#"$s=New-Object -ComObject WScript.Shell;$l=$s.CreateShortcut((Join-Path ([Environment]::GetFolderPath('CommonDesktopDirectory')) 'Mahoutsukai no Yoru - Francais et Voix.lnk'));$l.WorkingDirectory=$p;$l.TargetPath=Join-Path $p '魔法使いの夜.exe';$l.IconLocation=$l.TargetPath+',0';$l.Save()"#,
        target,
    )?;
    Ok(())
}

// Elevate through the Windows consent dialog before opening the installer.
pub fn request_admin() -> Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::{
        Shell::{IsUserAnAdmin, ShellExecuteW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    };
    if unsafe { IsUserAnAdmin() } != 0 {
        return Ok(true);
    }
    let path = std::env::current_exe()?
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            windows_sys::w!("runas"),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    ensure!(
        result > 32,
        "Les droits administrateur ont été refusés ou le démarrage a échoué."
    );
    Ok(false)
}

pub fn configure_japanese() -> Result<()> {
    powershell(
        include_str!("../sources/configure-japanese.ps1"),
        Path::new("."),
    )?;
    Ok(())
}

pub fn japanese() -> bool {
    unsafe { windows_sys::Win32::Globalization::GetACP() == 932 }
}
pub fn launch(target: &Path) -> Result<()> {
    ensure!(
        japanese(),
        "Configure les programmes non Unicode en japonais et redémarre Windows."
    );
    Command::new(target.join(format!("{GAME}.exe")))
        .current_dir(target)
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_overlays_into_a_new_directory() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().join("package");
        fs::create_dir(&base)?;
        let source = tmp.path().join("disc");
        fs::create_dir(&source)?;
        fs::write(source.join(format!("{GAME}.exe")), b"game")?;
        let target = tmp.path().join("destination");
        fs::create_dir(&target)?;
        let mut assets = assets::manifest(&base);
        assets.truncate(2);
        let patch = &mut assets[1];
        fs::create_dir_all(patch.path.parent().unwrap())?;
        fs::write(&patch.path, b"patch")?;
        patch.len = 5;
        patch.hash = format!("{:X}", Sha256::digest(b"patch"));
        stage(&base, &target, &source, &assets, &|_, _| {})?;
        assert_eq!(fs::read(target.join(format!("{GAME}.exe")))?, b"game");
        assert_eq!(
            fs::read(target.join(r"savedata\patches\patch1.xp3"))?,
            b"patch"
        );
        assert!(stage(&base, &target, &source, &assets, &|_, _| {}).is_err());
        let raced = tmp.path().join("raced");
        fs::create_dir(&raced)?;
        assert!(
            stage(&base, &raced, &source, &assets, &|_, _| {
                fs::write(raced.join("save"), b"new save").unwrap();
            })
            .is_err()
        );
        assert_eq!(fs::read(raced.join("save"))?, b"new save");
        Ok(())
    }
    #[test]
    fn safeguards() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        let base = root.join("installateur");
        let target = suggest(root)?;
        fs::create_dir(&target)?;
        fs::write(target.join("save"), b"user")?;
        assert!(validate(&target, &base).is_err());
        assert!(validate(&base.join("game"), &base).is_err());
        assert!(validate(&root.join(".mahoyo-cache"), &base).is_err());
        assert!(validate(root, &base).is_err());
        assert!(suggest(root)?.ends_with("Mahoyo-FR-Voix (2)"));
        assert_eq!(fs::read(target.join("save"))?, b"user");
        Ok(())
    }
    #[test]
    fn corrupted_patch_aborts_before_publish() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        let source = root.join("disc");
        fs::create_dir(&source)?;
        fs::write(source.join(format!("{GAME}.exe")), b"game")?;
        let mut assets = assets::manifest(root);
        assets.truncate(2);
        assets[1].path = source.join(format!("{GAME}.exe"));
        let target = root.join("target");
        assert!(stage(&root.join("base"), &target, &source, &assets, &|_, _| {}).is_err());
        assert!(!target.exists());
        assert_eq!(root.read_dir()?.count(), 1);
        Ok(())
    }
    #[test]
    #[ignore = "Full offline install using MAHOYO_TEST_PACKAGE and an isolated scratch drive"]
    fn offline_install() -> Result<()> {
        let root = PathBuf::from(std::env::var_os("MAHOYO_TEST_SCRATCH").unwrap());
        let tmp = tempfile::tempdir_in(root)?;
        let base = PathBuf::from(std::env::var_os("MAHOYO_TEST_PACKAGE").unwrap());
        let target = tmp.path().join("game");
        println!("{}", install(&base, &target, None, &|_, _| {})?);
        for a in assets::manifest(&base.join("donnees")).into_iter().skip(1) {
            let path = if a.name.starts_with("patches\\") {
                target.join("savedata").join(&a.name)
            } else {
                target.join(a.name.strip_prefix("correctifs\\").unwrap())
            };
            assert!(Asset { path, ..a }.valid()?);
        }
        Ok(())
    }
}
