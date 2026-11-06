use super::*;
use std::net::TcpListener;
use std::thread;

fn sample(dir: &Path, name: &str, bytes: &[u8]) -> Asset {
    Asset {
        name: name.into(),
        len: bytes.len() as u64,
        hash: format!("{:X}", Sha256::digest(bytes)),
        path: dir.join(name),
    }
}

#[test]
fn embedded_manifest_matches_supported_package() {
    let files = manifest(Path::new("donnees"));
    assert_eq!(files.len(), 15);
    assert_eq!(files[0].name, "MAHOYO.ISO");
    for (i, file) in files.iter().enumerate().take(9).skip(1) {
        assert_eq!(file.name, format!("patches\\patch{i}.xp3"));
    }
    for f in files {
        assert!(f.len > 0 && f.hash.len() == 64 && f.hash.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}

#[test]
fn assembled_iso_is_published_only_after_verification() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let iso = sample(dir.path(), "image", b"AB");
    let parts = [
        sample(dir.path(), "one", b"A"),
        sample(dir.path(), "two", b"B"),
    ];
    fs::write(&parts[0].path, b"A")?;
    fs::write(&parts[1].path, b"X")?;
    assert!(assemble(&iso, &parts, &|_, _| {}).is_err());
    assert!(!iso.path.exists());
    fs::write(&parts[1].path, b"B")?;
    assemble(&iso, &parts, &|_, _| {})?;
    assert!(iso.valid()?);
    Ok(())
}

#[test]
fn resumes_and_handles_servers_that_ignore_range() -> Result<()> {
    for resume in [true, false] {
        let dir = tempfile::tempdir()?;
        let a = sample(dir.path(), "patch", b"abcdef");
        fs::write(a.path.with_added_extension("part"), b"abc")?;
        let server = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}/file", server.local_addr()?);
        let task = thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let (mut stream, _) = server.accept().unwrap();
            let headers = BufReader::new(&stream)
                .lines()
                .map_while(std::io::Result::ok)
                .take_while(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
                .to_lowercase();
            assert!(headers.contains("range: bytes=3-"));
            let reply = if resume {
                "206 Partial Content\r\nContent-Range: bytes 3-5/6\r\nContent-Length: 3\r\n\r\ndef"
            } else {
                "200 OK\r\nContent-Length: 6\r\n\r\nabcdef"
            };
            write!(stream, "HTTP/1.1 {reply}").unwrap();
        });
        download(&Client::builder().no_proxy().build()?, &url, &a, &|_| {})?;
        task.join().unwrap();
        assert!(a.valid()?);
    }
    Ok(())
}

#[test]
#[ignore = "Downloads one MoonTrad patch and the official Windows fix"]
fn small_online_payloads() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let files: Vec<_> = manifest(tmp.path())
        .into_iter()
        .filter(|a| {
            a.name.ends_with("patch2.xp3") || a.name.contains("plugin") || a.name.ends_with(".cf")
        })
        .collect();
    prepare_patches(&Client::new(), tmp.path(), files.clone(), &|_, _| {})?;
    for a in files {
        assert!(a.valid()?, "{}", a.name);
    }
    Ok(())
}

#[test]
#[ignore = "Rebuilds the three disc corrections from a mounted reference ISO"]
fn mounted_disc_corrections() -> Result<()> {
    let disc = PathBuf::from(std::env::var_os("MAHOYO_TEST_DISC").expect("MAHOYO_TEST_DISC"));
    let tmp = tempfile::tempdir()?;
    let assets = manifest(tmp.path());
    disc_corrections(&assets, &disc)?;
    for a in &assets[12..] {
        assert!(a.valid()?);
    }
    Ok(())
}

#[test]
fn two_downloads_can_progress_before_either_finishes() -> Result<()> {
    use std::io::{BufRead, BufReader};
    let dir = tempfile::tempdir()?;
    let a = sample(dir.path(), "one", b"x");
    let b = sample(dir.path(), "two", b"x");
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/", listener.local_addr()?);
    let server = thread::spawn(move || -> std::io::Result<()> {
        let mut streams = Vec::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept()?;
            for line in BufReader::new(&stream).lines() {
                if line?.is_empty() {
                    break;
                }
            }
            streams.push(stream);
        }
        for mut stream in streams {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx")?;
        }
        Ok(())
    });
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    download_batch(&client, &[(&a, url.clone()), (&b, url)], &|_, _| {})?;
    server.join().unwrap()?;
    assert!(a.valid()? && b.valid()?);
    Ok(())
}

#[test]
#[ignore = "Compare serial and paired transfers of two public MoonTrad patches"]
fn compare_public_transfers() -> Result<()> {
    let client = Client::builder().timeout(Duration::from_secs(90)).build()?;
    for parallel in [false, true, true, false] {
        let tmp = tempfile::tempdir()?;
        let assets = manifest(tmp.path());
        let jobs:Vec<_>=[1,8].into_iter().map(|i|(&assets[i],format!("https://raw.githubusercontent.com/IDerr/mahoyo-french/44b5691a9ad1f5b8858240028c23a605cd56cc0a/Release/Patch/patch{i}.xp3"))).collect();
        let start = std::time::Instant::now();
        for group in jobs.chunks(if parallel { 2 } else { 1 }) {
            download_batch(&client, group, &|_, _| {})?;
        }
        println!(
            "{} files at once: {:?}",
            if parallel { 2 } else { 1 },
            start.elapsed()
        );
        for (asset, _) in jobs {
            assert!(asset.valid()?);
        }
    }
    Ok(())
}
