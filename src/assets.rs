use anyhow::{Context, Result, bail, ensure};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::NamedTempFile;

pub const GAME: &str = "魔法使いの夜";
pub type Report<'a> = dyn Fn(u32, &str) + 'a;
const PART_HASHES: [&str; 4] = [
    "D274318A6C6C526EBC861B24B092144B2377D516713439EC47CAB57853912BD4",
    "A3FAFCDFC8A50D1D74DCC18F41BCC779F6FC08964A720852904D9DCDBC0663DA",
    "0A94231980C71A9A366B6C79E996D64DADDD3587E4720DBE2D28C8DD25E7C09E",
    "9E2FAC470D5440C1DC5332ED343AFF9C438478F9045C73CDEEC3D1127042EC3B",
];
const DRIVE: [&str; 4] = [
    "1NQDf-kDc1ZZu_QHwp_NOQclKhiJLaBWd",
    "1xjA8V6NAYQ9JMHCcDK7bJENcoyUOZpfK",
    "1e4VQruMe13Ap1kKuLv5GymfsEdPhKzGt",
    "1QJCVXtuBACZiEQNXDgjGsoNZttxBl9jN",
];

#[derive(Clone)]
pub struct Asset {
    pub name: String,
    pub len: u64,
    pub hash: String,
    pub path: PathBuf,
}

pub fn manifest(root: &Path) -> Vec<Asset> {
    // The release manifest is compiled into the executable, including every allowed relative path.
    include_str!("../donnees/manifest.tsv")
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().unwrap();
            Asset {
                name: name.into(),
                len: fields.next().unwrap().parse().unwrap(),
                hash: fields.next().unwrap().into(),
                path: root.join(name),
            }
        })
        .collect()
}

pub fn no_links(path: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(m) => ensure!(
                m.file_attributes() & 0x400 == 0,
                "Lien ou jonction interdit : {}",
                part.display()
            ),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn digest(reader: &mut impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:X}", hash.finalize()))
}

impl Asset {
    pub fn valid(&self) -> Result<bool> {
        no_links(&self.path)?;
        match File::open(&self.path) {
            Ok(mut f) => Ok(f.metadata()?.len() == self.len && digest(&mut f)? == self.hash),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    fn save(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() as u64 == self.len && format!("{:X}", Sha256::digest(bytes)) == self.hash,
            "Correctif invalide : {}",
            self.name
        );
        let dir = self.path.parent().unwrap();
        no_links(&self.path)?;
        fs::create_dir_all(dir)?;
        let mut tmp = NamedTempFile::new_in(dir)?;
        tmp.write_all(bytes)?;
        tmp.persist(&self.path)?;
        Ok(())
    }
}

fn transfer(
    client: &Client,
    url: &str,
    asset: &Asset,
    part: &mut File,
    progress: &dyn Fn(u64),
) -> Result<()> {
    let mut offset = part.metadata()?.len();
    let mut req = client.get(url);
    if offset > 0 {
        req = req.header("Range", format!("bytes={offset}-"))
    }
    let mut res = req.send()?.error_for_status()?;
    if res.status().as_u16() == 206 {
        ensure!(
            res.headers()
                .get("Content-Range")
                .and_then(|v| v.to_str().ok())
                == Some(format!("bytes {offset}-{}/{}", asset.len - 1, asset.len).as_str()),
            "Réponse de reprise incorrecte."
        );
    } else {
        ensure!(res.status().as_u16() == 200, "Réponse inattendue.");
        offset = 0;
    }
    ensure!(
        !res.headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .starts_with("text/html"),
        "Le fournisseur demande une confirmation ou a atteint son quota."
    );
    ensure!(
        res.content_length()
            .is_none_or(|len| len == asset.len - offset),
        "Taille reçue incorrecte : {}",
        asset.name
    );
    part.set_len(offset)?;
    part.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0; 1024 * 1024];
    let mut writer = io::BufWriter::with_capacity(1024 * 1024, part);
    progress(offset);
    let mut last = offset;
    loop {
        let n = res.read(&mut buf)?;
        if n == 0 {
            break;
        }
        offset += n as u64;
        ensure!(offset <= asset.len, "Téléchargement trop volumineux.");
        writer.write_all(&buf[..n])?;
        if offset - last >= 1024 * 1024 || offset == asset.len {
            progress(offset);
            last = offset;
        }
    }
    writer.flush()?;
    ensure!(offset == asset.len, "Téléchargement interrompu.");
    Ok(())
}

fn download(client: &Client, url: &str, asset: &Asset, progress: &dyn Fn(u64)) -> Result<()> {
    let temp = asset.path.with_added_extension("part");
    no_links(&temp)?;
    fs::create_dir_all(temp.parent().unwrap())?;
    let mut part = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&temp)?;
    if part.metadata()?.len() > asset.len {
        part.set_len(0)?;
    }
    for attempt in 0..3 {
        if part.metadata()?.len() == asset.len {
            break;
        }
        match transfer(client, url, asset, &mut part, progress) {
            Ok(()) => break,
            Err(e) if attempt == 2 => return Err(e.context(asset.name.clone())),
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
    part.rewind()?;
    if digest(&mut part)? != asset.hash {
        part.set_len(0)?;
        bail!(
            "Empreinte incorrecte : {}. Relance le téléchargement.",
            asset.name
        );
    }
    drop(part);
    fs::rename(temp, &asset.path)?;
    Ok(())
}

// Only independent files run together. The main worker serializes UI updates.
fn download_batch(client: &Client, jobs: &[(&Asset, String)], report: &Report) -> Result<()> {
    let total: u64 = jobs.iter().map(|(asset, _)| asset.len).sum();
    let mut complete = 0;
    for pair in jobs.chunks(2) {
        std::thread::scope(|scope| -> Result<()> {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut workers = Vec::new();
            for (i, (asset, url)) in pair.iter().enumerate() {
                let tx = tx.clone();
                workers.push(scope.spawn(move || {
                    download(client, url, asset, &|bytes| {
                        let _ = tx.send((i, bytes));
                    })
                }));
            }
            drop(tx);
            let mut offsets = [0; 2];
            for (i, bytes) in rx {
                offsets[i] = bytes;
                let done = complete + offsets.iter().sum::<u64>();
                report(
                    (20 * done / total.max(1)) as u32,
                    &format!("Téléchargement · {} / {} Mio", done >> 20, total >> 20),
                );
            }
            for worker in workers {
                worker
                    .join()
                    .map_err(|_| anyhow::anyhow!("Transfert interrompu."))??;
            }
            Ok(())
        })?;
        complete += pair.iter().map(|(a, _)| a.len).sum::<u64>();
    }
    Ok(())
}

fn pieces(cache: &Path) -> Vec<Asset> {
    PART_HASHES
        .iter()
        .enumerate()
        .map(|(i, hash)| {
            let name = format!("MAHOYO.ISO.part{}", i + 1);
            Asset {
                path: cache.join("cache-iso").join(&name),
                name,
                len: if i == 3 { 1610540800 } else { 1900000000 },
                hash: hash.to_string(),
            }
        })
        .collect()
}

fn assemble(iso: &Asset, parts: &[Asset], report: &Report) -> Result<()> {
    let mut tmp = NamedTempFile::new_in(iso.path.parent().unwrap())?;
    let mut hash = Sha256::new();
    let mut total = 0;
    let mut buf = vec![0; 1024 * 1024];
    for p in parts {
        let mut input = File::open(&p.path)?;
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            ensure!(total <= iso.len, "Morceaux ISO trop volumineux.");
            tmp.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
            report((total * 20 / iso.len) as u32, "Assemblage de l’ISO…");
        }
    }
    ensure!(
        total == iso.len && format!("{:X}", hash.finalize()) == iso.hash,
        "ISO assemblée incorrecte."
    );
    tmp.persist(&iso.path)?;
    Ok(())
}

pub fn prepare(
    base: &Path,
    cache: &Path,
    local: Option<&Path>,
    report: &Report,
) -> Result<Vec<Asset>> {
    let mut assets = manifest(&base.join("donnees"));
    let mut missing = Vec::new();
    for a in &mut assets {
        report(0, &format!("Vérification de {}", a.name));
        if a.name == "MAHOYO.ISO"
            && let Some(path) = local
        {
            a.path = path.into();
            ensure!(
                a.valid()?,
                "L’ISO sélectionnée ne correspond pas à Mahoyo PC 2012."
            );
            continue;
        }
        if a.valid()? {
            continue;
        }
        a.path = cache.join(&a.name);
        if !a.valid()? {
            missing.push(a.clone());
        }
    }
    let iso_missing = missing.iter().any(|a| a.name == "MAHOYO.ISO");
    let mut parts = Vec::new();
    let mut needed = 7308540406
        + assets.iter().skip(1).map(|a| a.len).sum::<u64>()
        + 2 * 64 * 1024 * 1024
        + missing.iter().map(|a| a.len).sum::<u64>();
    if iso_missing {
        for p in pieces(cache) {
            let valid = p.valid()?;
            if !valid {
                needed += p.len;
            }
            parts.push((p, valid));
        }
    }
    ensure!(
        fs2::available_space(cache)? >= needed,
        "Espace insuffisant sur {}. Prévoir {} Go libres.",
        cache.display(),
        needed.div_ceil(1 << 30)
    );
    if missing.is_empty() {
        return Ok(assets);
    }
    let client = Client::builder()
        .user_agent("MahoyoInstaller/0.2")
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(6 * 3600))
        .build()?;
    if iso_missing {
        let jobs: Vec<_> = parts
            .iter()
            .filter(|(_, valid)| !*valid)
            .map(|(p, _)| {
                (
                    p,
                    format!(
                        "https://github.com/TrixWyzzi/mahoyo-installer/releases/download/v0.2.4/{}",
                        p.name
                    ),
                )
            })
            .collect();
        download_batch(&client, &jobs, report)?;
        let all: Vec<_> = parts.into_iter().map(|x| x.0).collect();
        assemble(&assets[0], &all, report)?;
        for p in all {
            fs::remove_file(p.path)?;
        }
    }
    prepare_patches(&client, cache, missing, report)?;
    Ok(assets)
}

fn prepare_patches(
    client: &Client,
    cache: &Path,
    missing: Vec<Asset>,
    report: &Report,
) -> Result<()> {
    let mut jobs = Vec::new();
    for a in &missing {
        if let Some(name) = a.name.strip_prefix("patches\\") {
            let index: usize = name
                .trim_start_matches("patch")
                .trim_end_matches(".xp3")
                .parse()?;
            let url = if (4..=7).contains(&index) {
                format!(
                    "https://drive.usercontent.google.com/download?id={}&export=download&authuser=0&confirm=t",
                    DRIVE[index - 4]
                )
            } else {
                format!(
                    "https://raw.githubusercontent.com/IDerr/mahoyo-french/44b5691a9ad1f5b8858240028c23a605cd56cc0a/Release/Patch/{name}"
                )
            };
            jobs.push((a, url));
        }
    }
    download_batch(client, &jobs, report)?;
    let mut archive = None;
    for a in missing {
        if a.name.starts_with("correctifs\\plugin\\") {
            if archive.is_none() {
                let fix = Asset {
                    name: "forWin8x64.zip".into(),
                    len: 115948,
                    hash: "BB97B8C5CC225F020AA9C6376A671FE0EF109F61A4D32E74510690B7A7242A9C".into(),
                    path: cache.join("forWin8x64.zip"),
                };
                if !fix.valid()? {
                    download(
                        client,
                        "https://www.typemoon.com/users/download/WOHN/forWin8x64.zip",
                        &fix,
                        &|_| report(20, "Téléchargement du correctif Windows…"),
                    )?;
                }
                archive = Some(zip::ZipArchive::new(File::open(fix.path)?)?);
            }
            let zip = archive.as_mut().unwrap();
            let expected = format!(
                "forWin8x64/{}",
                a.path.file_name().unwrap().to_string_lossy()
            );
            let mut found = false;
            for i in 0..zip.len() {
                let entry = zip.by_index(i)?;
                if encoding_rs::SHIFT_JIS.decode(entry.name_raw()).0 == expected
                    || entry.name() == expected
                {
                    ensure!(
                        !found && entry.size() == a.len,
                        "Archive du correctif inattendue."
                    );
                    let mut bytes = Vec::new();
                    entry.take(a.len + 1).read_to_end(&mut bytes)?;
                    a.save(&bytes)?;
                    found = true;
                }
            }
            ensure!(found, "Fichier absent du correctif : {}", a.name);
        } else if a.name.ends_with(".cf") {
            let escaped = "$(exepath)/savedata"
                .bytes()
                .map(|c| format!("\\x{c:02x}"))
                .collect::<String>();
            a.save(format!("datapath=\"{escaped}\"").as_bytes())?;
        }
    }
    Ok(())
}

pub fn disc_corrections(assets: &[Asset], disc: &Path) -> Result<()> {
    for a in assets.iter().filter(|a| {
        a.name.starts_with("correctifs\\")
            && !a.name.contains("\\plugin\\")
            && !a.name.ends_with(".cf")
    }) {
        if a.valid()? {
            continue;
        }
        let bytes = if a.name.ends_with(".ini") {
            let input = fs::read(disc.join("sigchk.ini"))?;
            let (text, _, _) = encoding_rs::SHIFT_JIS.decode(&input);
            let (header, key) = text
                .split_once("-----BEGIN")
                .context("Configuration du vérificateur incorrecte.")?;
            let header = header
                .replace("[cr]※ ", "[cr]")
                .replace("[cr]　　この場合は、", "")
                .replace(
                    "caption=ファイル破損チェックツール",
                    "caption=魔法使いの夜 ファイル破損チェックツール",
                )
                .replace("\r\n", "\r\r\n");
            encoding_rs::SHIFT_JIS
                .encode(&format!("{header}-----BEGIN{key}"))
                .0
                .into_owned()
        } else if a.name.ends_with(".exe") {
            fs::read(disc.join("sigchk.exe"))?
        } else {
            fs::read(disc.parent().unwrap().join("readme.txt"))?
        };
        a.save(&bytes)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "asset_tests.rs"]
mod tests;
