use modio::{Credentials, Modio};
use modio::download::DownloadAction;
use modio::filter::In;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

use clap::{Parser, Subcommand};

use dotenv::dotenv;
use tokio::io::AsyncWriteExt;
use std::env;
use futures::TryStreamExt;
use anyhow::Result;

use u4pak::Pak;

use std::io::Read;
use std::path::Path;
use std::fs;

use indicatif::ProgressBar;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Search {
        #[clap(value_parser)]
        name: Option<String>
    },
    GetMods,
    UpdateModFilesLocal,
    ListFiles {
        #[clap(value_parser)]
        zip: Option<std::path::PathBuf>
    },
    Test,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let options = SqlitePoolOptions::new().max_connections(1);
    let pool = options.connect(&env::var("DATABASE_URL")?).await?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Search { name } => {
        },
        Commands::GetMods => {
            get_mods(&pool).await?;
        },
        Commands::UpdateModFilesLocal => {
            update_pack_files_local(&pool).await?;
        },
        Commands::ListFiles { zip } => {
            if let Some(path) = zip {
                list_zip_files(&path)?;
            } else {
                for dir_entry in fs::read_dir("mods")? {
                    let path = &dir_entry?.path();
                    match list_zip_files(path) {
                        Ok(files) => for file in files { println!("{} {}", path.display(), file); },
                        Err(e) => println!("{} {}", path.display(), e)
                    }
                }
            }
        },
        Commands::Test => {
        },
    }

    Ok(())
}


fn list_zip_files(path: &Path) -> Result<Vec<String>, PakError> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);

    let mut archive = zip::ZipArchive::new(reader)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_file() && file.name().to_lowercase().ends_with(".pak") {
            return list_files(&mut file);
        }
    }
    Err(PakError::MissingPakFile)
}

#[derive(Debug)]
enum PakError {
    ErrorReadingPak { e: u4pak::Error },
    MissingMountPoint,
    MissingPakFile,
    AssetPathError { mount_point: String, asset_path: String },
    StripPrefixError { e: std::path::StripPrefixError },
    ZipError(zip::result::ZipError),
    IoError(std::io::Error),
}

impl From<zip::result::ZipError> for PakError {
    fn from(e: zip::result::ZipError) -> PakError {
        PakError::ZipError(e)
    }
}
impl From<std::io::Error> for PakError {
    fn from(e: std::io::Error) -> PakError {
        PakError::IoError(e)
    }
}
impl std::error::Error for PakError {}

impl std::fmt::Display for PakError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PakError::ErrorReadingPak { e } => write!(f, "{self:?}: {e}"),
            PakError::MissingMountPoint
                | PakError::MissingPakFile => write!(f, "{self:?}"),
            PakError::AssetPathError { mount_point, asset_path } => write!(f, "{self:?}: mount point: {mount_point:?} asset path: {asset_path:?}"),
            PakError::StripPrefixError { e } => write!(f, "{self:?}: {e}"),
            PakError::ZipError ( e ) => write!(f, "{self:?}: {e}"),
            PakError::IoError ( e ) => write!(f, "{self:?}: {e}"),
        }
    }
}

fn list_files(file: &mut zip::read::ZipFile) -> Result<Vec<String>, PakError> {
    let mut buffer: Vec<u8> = vec![];
    file.read_to_end(&mut buffer)?;
    let mut cursor = std::io::Cursor::new(buffer);
    let pak = Pak::from_reader(&mut cursor, Default::default()).map_err(|e| PakError::ErrorReadingPak {e})?;
    let index = pak.index();
    let mount_point = index.mount_point().ok_or(PakError::MissingMountPoint)?;

    pak.index().records().iter().map(|record| {
        let mut path = std::path::PathBuf::new();
        let asset_path = record.filename();
        path.push(mount_point);
        path.push(asset_path);
        let path_str = path
            .as_path()
            .strip_prefix("../../..").map_err(|e| PakError::StripPrefixError { e })?
            .to_str().ok_or_else(|| PakError::AssetPathError { mount_point: mount_point.to_string(), asset_path: asset_path.to_string() })?;
        Ok(path_str.to_owned())
    }).collect()
}

async fn get_mods(pool: &SqlitePool) -> Result<()> {
    let modio = Modio::new(
        Credentials::with_token(&env::var("MODIO_KEY")?, &env::var("MODIO_ACCESS_TOKEN")?),
    )?;

    let drg = 2475;

    //let mods = modio.game(drg).mods().search(Filter::default().limit(1)).collect().await?;

    println!("Grabbing mod list...");
    let filter = modio::mods::filters::Visible::_in(vec![0, 1]);
    let mods = modio.game(drg).mods().search(filter).collect().await?;
    println!("Mod list obtained");

    let multi_bar = indicatif::MultiProgress::new();
    let mod_bar = multi_bar.add(ProgressBar::new(mods.len().try_into().unwrap()));
    for m in mods {
        //println!("{}. {} {}", m.id, m.name, m.name_id);
        update_mod(&multi_bar, pool, &modio, m).await?;
        mod_bar.inc(1);
    }
    mod_bar.finish();

    Ok(())
}

async fn update_mod(multi_bar: &indicatif::MultiProgress, pool: &SqlitePool, modio: &Modio, m: modio::mods::Mod) -> Result<()> {
    let mut tx = pool.begin().await?;

    //let id_modfile: Option<u32> = m.modfile.as_ref().map(|f| f.id);
    sqlx::query!("INSERT INTO mod(id_mod, name, name_id, summary, description)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(id_mod) DO
                    UPDATE SET
                        name = excluded.name,
                        name_id = excluded.name_id,
                        summary = excluded.summary,
                        description = excluded.summary;", m.id, m.name, m.name_id, m.summary, m.description)
        .execute(&mut tx)
        .await?;

    let modfile = sqlx::query!("SELECT id_modfile FROM mod WHERE id_mod = ?", m.id).fetch_one(&mut tx).await?.id_modfile.map(|id| id as u32);

    if m.modfile.as_ref().map(|f| f.id) != modfile {
        if let Some(file) = m.modfile {
            let path = Path::new("mods").join(format!("{}.zip", file.filehash.md5));

            let id_modfile = file.id;
            let date = chrono::DateTime::<chrono::Utc>::from_utc(chrono::NaiveDateTime::from_timestamp_opt(file.date_added.try_into().unwrap(), 0).unwrap(), chrono::Utc).to_rfc3339();
            sqlx::query!("INSERT INTO modfile(id_modfile, id_mod, date_added, hash_md5, filename, version, changelog)
                         VALUES (?, ?, ?, ?, ?, ?, ?)
                         ON CONFLICT(id_modfile) DO
                            UPDATE SET
                                id_modfile = excluded.id_modfile,
                                id_mod = excluded.id_mod,
                                date_added = excluded.date_added,
                                hash_md5 = excluded.hash_md5,
                                filename = excluded.filename,
                                version = excluded.version,
                                changelog = excluded.changelog;", id_modfile, m.id, date, file.filehash.md5, file.filename, file.version, file.changelog).execute(&mut tx).await?;

            sqlx::query!("UPDATE mod SET id_modfile = ? WHERE id_mod = ?", id_modfile, m.id).execute(&mut tx).await?;

            if !std::path::Path::new(&path).exists() {
                multi_bar.println(format!("Downloading mod {}", m.id))?;
                let download_bar = multi_bar.add(indicatif::ProgressBar::new(file.filesize));
                download_bar.set_style(indicatif::ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")?.progress_chars("#>-"));

                let mut stream = Box::pin(modio.download(DownloadAction::FileObj(Box::new(file))).stream());
                let mut file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&path)
                    .await?;
                while let Some(bytes) = stream.try_next().await? {
                    file.write_all(&bytes).await?;
                    download_bar.inc(bytes.len() as u64);
                }

                multi_bar.remove(&download_bar);
            }

            sqlx::query!("DELETE FROM pack_file WHERE id_modfile = ?", id_modfile).execute(&mut tx).await?;

            let res = list_zip_files(&path);
            match res {
                Ok(files) => {
                    for file in files {
                        let path = std::path::Path::new(&file);
                        let extension = path.extension().and_then(std::ffi::OsStr::to_str);
                        let name = path.file_stem().and_then(std::ffi::OsStr::to_str);
                        let path_no_extension = if let Some(ext) = extension {
                            file.strip_suffix(&ext).unwrap()
                        } else {
                            &file
                        };
                        sqlx::query!("INSERT INTO pack_file(id_modfile, path, path_no_extension, extension, name)
                                     VALUES (?, ?, ?, ?, ?)", id_modfile, file, path_no_extension, extension, name).execute(&mut tx).await?;
                    }
                },
                Err(e) => {
                    multi_bar.println(format!("Error analyzing {}: {}", m.id, e))?;
                }
            }
        } else {
            sqlx::query!("UPDATE mod SET id_modfile = NULL WHERE id_mod = ?", m.id).execute(&mut tx).await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

async fn update_pack_files_local(pool: &SqlitePool) -> Result<()> {
    let modfiles = sqlx::query!("SELECT id_modfile, hash_md5 FROM modfile").fetch_all(pool).await?;

    use futures::stream::StreamExt;

    let bar = indicatif::ProgressBar::new(modfiles.len().try_into().unwrap());
    let mut stream = futures::stream::iter(modfiles.into_iter().map(|modfile| {
        tokio::task::spawn_blocking(move || (modfile.id_modfile, get_pack_files(modfile.id_modfile, modfile.hash_md5)) )
    })).buffer_unordered(std::thread::available_parallelism()?.get());

    use sqlx::{Executor, Statement};
    let delete = pool.prepare("DELETE FROM pack_file WHERE id_modfile = ?").await?;
    let insert = pool.prepare("INSERT INTO pack_file(id_modfile, path, path_no_extension, extension, name) VALUES (?, ?, ?, ?, ?)").await?;

    while let Some(item) = stream.next().await {
        let (id, pack_files) = item?;
        match pack_files {
            Ok(pack_files) => {
                let mut tx = pool.begin().await?;
                delete.query().bind(id).execute(&mut tx).await?;
                for file in pack_files {
                    insert
                        .query()
                        .bind(file.id_modfile)
                        .bind(file.path)
                        .bind(file.path_no_extension)
                        .bind(file.extension)
                        .bind(file.name)
                        .execute(&mut tx)
                        .await?;
                }
                tx.commit().await?;
            },
            Err(err) => {
                bar.println(format!("Error analyzing modfile_id {id}: {err}"));
            }
        }
        bar.inc(1);
    }
    bar.finish();

    Ok(())
}

async fn update_pack_file_local(pool: SqlitePool, id: i64, md5: String) -> Result<()> {
    let mut tx = pool.begin().await?;
    let path = Path::new("mods").join(format!("{md5}.zip"));

    sqlx::query!("DELETE FROM pack_file WHERE id_modfile = ?", id).execute(&mut tx).await?;

    use sqlx::{Executor, Statement};

    let insert = tx.prepare("INSERT INTO pack_file(id_modfile, path, path_no_extension, extension, name) VALUES (?, ?, ?, ?, ?)").await?;

    let res = list_zip_files(&path);
    match res {
        Ok(files) => {
            for file in files {
                let path = std::path::Path::new(&file);
                let extension = path.extension().and_then(std::ffi::OsStr::to_str);
                let name = path.file_stem().and_then(std::ffi::OsStr::to_str);
                let path_no_extension = if let Some(ext) = extension {
                    file.strip_suffix(&ext).unwrap()
                } else {
                    &file
                };
                insert
                    .query()
                    .bind(id)
                    .bind(&file)
                    .bind(path_no_extension)
                    .bind(extension)
                    .bind(name)
                    .execute(&mut tx).await?;
            }
        },
        Err(e) => {
            println!("Error analyzing modfile {id}: {e}");
        }
    }

    tx.commit().await?;

    Ok(())
}

struct PackFile {
    id_modfile: i64,
    path: String,
    path_no_extension: String,
    name: Option<String>,
    extension: Option<String>,
}

fn get_pack_files(id_modfile: i64, md5: String) -> Result<Vec<PackFile>> {
    let path = Path::new("mods").join(format!("{md5}.zip"));

    let files = list_zip_files(&path)?;
    Ok(files.into_iter().map(|path| {
        let p = std::path::Path::new(&path);
        let extension = p.extension().and_then(std::ffi::OsStr::to_str).map(|s| s.to_string());
        let name = p.file_stem().and_then(std::ffi::OsStr::to_str).map(|s| s.to_string());
        let path_no_extension = if let Some(ext) = &extension {
            path.strip_suffix(ext).unwrap().to_string()
        } else {
            path.to_owned()
        };
        PackFile {
            id_modfile,
            path,
            path_no_extension,
            name,
            extension,
        }
    }).collect())
}
