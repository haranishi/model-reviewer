use std::collections::HashMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use model_reviewer_core::identity::ModelIdentity;

/// コピーによる移動は内容で追い、出力への例外にはファイル自体の識別も要求する。
#[derive(Clone, Default)]
pub(crate) struct FileHistory {
    opened: Vec<ModelIdentity>,
    outputs: HashMap<PathBuf, (ModelIdentity, FileStamp)>,
}

impl FileHistory {
    pub(crate) fn remember_opened(&mut self, identity: &ModelIdentity) {
        if !self.opened.contains(identity) {
            self.opened.push(identity.clone());
        }
    }

    pub(crate) fn remember_output(
        &mut self,
        path: &Path,
        identity: &ModelIdentity,
        stamp: Option<FileStamp>,
    ) {
        if let Some(stamp) = stamp {
            self.outputs
                .insert(path.to_owned(), (identity.clone(), stamp));
        } else {
            self.outputs.remove(path);
        }
    }

    pub(crate) fn is_written_output(&self, path: &Path, identity: &ModelIdentity) -> bool {
        fs::canonicalize(path)
            .ok()
            .and_then(|path| self.outputs.get(&path))
            .is_some_and(|(written, stamp)| {
                written == identity && FileStamp::read(path).as_ref() == Some(stamp)
            })
    }

    pub(crate) fn protects_contents(&self, path: &Path) -> io::Result<bool> {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err),
        };
        if !metadata.is_file()
            || !self
                .opened
                .iter()
                .any(|opened| opened.size_bytes == metadata.len())
        {
            return Ok(false);
        }
        let file = File::open(path)?;
        let stamp = FileStamp::from_file(&file);
        let identity = ModelIdentity::from_reader(file)?;
        if let Some(stamp) = stamp {
            stamp.check_path(path)?;
        }
        Ok(self.opened.contains(&identity) && !self.is_written_output(path, &identity))
    }
}

/// 出力の名前が外部で別のファイルに再利用されても、同じ出力と誤認しない。
/// 書き出し後にハンドルを持ち続けると、削除した領域の解放や次の置き換えを妨げる。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileStamp {
    volume: u64,
    id: u128,
    modified: SystemTime,
}

impl FileStamp {
    pub(crate) fn read(path: &Path) -> Option<Self> {
        Self::from_file(&File::open(path).ok()?)
    }

    pub(crate) fn check_path(&self, path: &Path) -> io::Result<()> {
        if Self::read(path).as_ref() != Some(self) {
            return Err(io::Error::other("照合中に保存先が変わりました"));
        }
        Ok(())
    }

    pub(crate) fn from_file(file: &File) -> Option<Self> {
        let metadata = file.metadata().ok()?;
        #[cfg(unix)]
        let (volume, id) = {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), u128::from(metadata.ino()))
        };
        #[cfg(windows)]
        let (volume, id) = windows_file_id(file)?;
        Some(Self {
            volume,
            id,
            modified: metadata.modified().ok()?,
        })
    }
}

#[cfg(windows)]
fn windows_file_id(file: &File) -> Option<(u64, u128)> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    #[derive(Default)]
    struct FileIdInfo {
        volume: u64,
        id: [u8; 16],
    }
    const FILE_ID_INFO_CLASS: i32 = 18;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            handle: *mut c_void,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
    }

    let mut info = FileIdInfo::default();
    // ハンドルは呼び出し中有効で、出力先は FILE_ID_INFO と同じ配置・大きさを持つ。
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FILE_ID_INFO_CLASS,
            (&raw mut info).cast(),
            size_of::<FileIdInfo>() as u32,
        )
    };
    (ok != 0).then_some((info.volume, u128::from_le_bytes(info.id)))
}
