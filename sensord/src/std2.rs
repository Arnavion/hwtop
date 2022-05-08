pub(crate) mod fs {
	pub(crate) fn canonicalize(path: &std::path::Path) -> Result<std::path::PathBuf, crate::Error> {
		crate::Error::with_path_context(path, |path| Ok(std::fs::canonicalize(path)?))
	}

	pub(crate) fn read_dir(path: &std::path::Path) -> Result<impl Iterator<Item = Result<std::fs::DirEntry, crate::Error>> + '_, crate::Error> {
		crate::Error::with_path_context(path, |path| {
			let iter = std::fs::read_dir(path)?;
			Ok(iter.map(|result| crate::Error::with_path_context(path, |_| Ok(result?))))
		})
	}
}
