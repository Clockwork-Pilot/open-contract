use std::fs;

pub(crate) fn exercise_copy_error_paths() -> Result<(), String> {
    let package = tempfile::tempdir().map_err(|error| error.to_string())?;
    let staging_plan = runner_manifest::ManifestPlan::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("isolation-package.fixture.yaml"),
    )
    .map_err(|error| error.to_string())?;
    let mut existing_write_plan = staging_plan.clone();
    existing_write_plan
        .manifest
        .filesystem
        .write
        .push("dataset.json".into());
    let existing_tree = super::StagedTree::create(&existing_write_plan)?;
    let staged_dataset =
        fs::read(existing_tree.root.join("dataset.json")).map_err(|error| error.to_string())?;
    let source_dataset = fs::read(staging_plan.package_root.join("dataset.json"))
        .map_err(|error| error.to_string())?;
    if staged_dataset != source_dataset {
        return Err("declared read/write file was unexpectedly overwritten".into());
    }

    let staging_error = super::StagedTree::create_with_runtime_using(
        &staging_plan,
        None,
        &mut |_| {
            Err(std::io::Error::other(
                "injected writable file creation failure",
            ))
        },
        &mut |path| fs::create_dir_all(path),
    )
    .err()
    .ok_or_else(|| "injected writable file creation unexpectedly succeeded".to_string())?;
    if !staging_error.contains("injected writable file creation failure") {
        return Err(format!("unexpected writable file error: {staging_error}"));
    }

    let mut directory_plan = staging_plan.clone();
    directory_plan.manifest.filesystem.write = vec!["new-directory".into()];
    let directory_error = super::StagedTree::create_tree_using(
        &directory_plan,
        true,
        None,
        &mut |path| fs::File::create(path),
        &mut |_| Err(std::io::Error::other("injected directory creation failure")),
    )
    .err()
    .ok_or_else(|| "injected writable directory creation unexpectedly succeeded".to_string())?;
    if !directory_error.contains("injected directory creation failure") {
        return Err(format!(
            "unexpected writable directory error: {directory_error}"
        ));
    }

    let mut parent_plan = staging_plan.clone();
    parent_plan.manifest.filesystem.write = vec!["new-parent/result.json".into()];
    let parent_error = super::StagedTree::create_tree_using(
        &parent_plan,
        true,
        None,
        &mut |path| fs::File::create(path),
        &mut |_| {
            Err(std::io::Error::other(
                "injected write parent creation failure",
            ))
        },
    )
    .err()
    .ok_or_else(|| "injected writable parent creation unexpectedly succeeded".to_string())?;
    if !parent_error.contains("injected write parent creation failure") {
        return Err(format!("unexpected writable parent error: {parent_error}"));
    }

    let mut mismatched_source_root = super::StagedTree::create(&staging_plan)?;
    mismatched_source_root.source_root = package.path().to_path_buf();
    let prefix_error = mismatched_source_root
        .stage_plan(
            &staging_plan,
            &mut |path| fs::File::create(path),
            &mut |path| fs::create_dir_all(path),
        )
        .err()
        .ok_or_else(|| "package outside the staging root unexpectedly staged".to_string())?;
    if !prefix_error.contains("outside the shared source root") {
        return Err(format!("unexpected package prefix error: {prefix_error}"));
    }

    let mut unresolved_root_plan = staging_plan.clone();
    unresolved_root_plan.package_root = std::path::PathBuf::from("relative/package");
    let common_root_error = super::StagedTree::create_tree_using(
        &unresolved_root_plan,
        true,
        None,
        &mut |path| fs::File::create(path),
        &mut |path| fs::create_dir_all(path),
    )
    .err()
    .ok_or_else(|| "unresolved package root unexpectedly staged".to_string())?;
    if !common_root_error.contains("must be absolute after manifest resolution") {
        return Err(format!(
            "unexpected unresolved package root error: {common_root_error}"
        ));
    }

    let source_file = package.path().join("source.txt");
    fs::write(&source_file, "source").map_err(|error| error.to_string())?;

    let file_create_error = super::create_declared_write_file_if_missing(
        &package.path().join("missing-parent/created.txt"),
        &mut |path| fs::File::create(path),
    )
    .expect_err("declared writable file creation error must propagate");
    if file_create_error.is_empty() {
        return Err("declared writable file creation error was empty".into());
    }

    let missing_root = package.path().join("missing-root");
    let error = super::copy_entry(
        &source_file,
        &missing_root,
        &package.path().join("copy.txt"),
    )
    .expect_err("unavailable package root must fail canonicalization");
    if error.is_empty() {
        return Err("missing package root error was empty".into());
    }

    for missing_source in [
        package.path().join("missing-inside.txt"),
        package.path().parent().unwrap().join("missing-outside.txt"),
    ] {
        let error = super::copy_entry(
            &missing_source,
            package.path(),
            &package.path().join("missing-source-copy.txt"),
        )
        .expect_err("unavailable source must fail canonicalization");
        if error.is_empty() {
            return Err("unavailable source error was empty".into());
        }
    }

    let blocked_parent = package.path().join("blocked-parent");
    fs::write(&blocked_parent, "not a directory").map_err(|error| error.to_string())?;
    if super::copy_entry(
        &source_file,
        package.path(),
        &blocked_parent.join("child.txt"),
    )
    .is_ok()
    {
        return Err("file destination under a file unexpectedly copied".into());
    }

    let destination_directory = package.path().join("destination-directory");
    fs::create_dir(&destination_directory).map_err(|error| error.to_string())?;
    if super::copy_entry(&source_file, package.path(), &destination_directory).is_ok() {
        return Err("copying a file onto a directory unexpectedly succeeded".into());
    }

    let source_directory = package.path().join("source-directory");
    fs::create_dir(&source_directory).map_err(|error| error.to_string())?;
    let directory_destination = package.path().join("directory-destination");
    fs::write(&directory_destination, "not a directory").map_err(|error| error.to_string())?;
    if super::copy_entry(&source_directory, package.path(), &directory_destination).is_ok() {
        return Err("copying a directory onto a file unexpectedly succeeded".into());
    }

    let read_error = super::copy_entry_with(
        &source_directory,
        package.path(),
        &package.path().join("read-directory-error"),
        &mut |_| Err(std::io::Error::other("injected directory read failure")),
    )
    .expect_err("directory read failure must propagate");
    if !read_error.contains("injected directory read failure") {
        return Err(format!("unexpected directory read error: {read_error}"));
    }

    let entry_error = super::copy_entry_with(
        &source_directory,
        package.path(),
        &package.path().join("read-entry-error"),
        &mut |_| {
            Ok(vec![Err(std::io::Error::other(
                "injected directory entry failure",
            ))])
        },
    )
    .expect_err("directory entry failure must propagate");
    if !entry_error.contains("injected directory entry failure") {
        return Err(format!("unexpected directory entry error: {entry_error}"));
    }

    #[cfg(unix)]
    {
        let external = tempfile::tempdir().map_err(|error| error.to_string())?;
        let external_file = external.path().join("outside.txt");
        fs::write(&external_file, "outside").map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink(&external_file, source_directory.join("escape"))
            .map_err(|error| error.to_string())?;
        if super::copy_entry(
            &source_directory,
            package.path(),
            &package.path().join("recursive-copy"),
        )
        .is_ok()
        {
            return Err("recursive copy accepted an escaping symlink".into());
        }

        let package_copy = tempfile::tempdir().map_err(|error| error.to_string())?;
        let external_child = tempfile::tempdir().map_err(|error| error.to_string())?;
        fs::write(
            external_child.path().join("contract.yaml"),
            "version: \"1\"\nname: child\ndatasets: [dataset.json]\ncoverage_sources: []\ncoverage:\n  minimum_line_percent: 100\nfilesystem:\n  read: [dataset.json, validator.txt]\n  write: []\n",
        )
        .map_err(|error| error.to_string())?;
        fs::write(external_child.path().join("dataset.json"), "{}")
            .map_err(|error| error.to_string())?;
        fs::write(external_child.path().join("validator.txt"), "validator")
            .map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink(
            external_child.path().join("contract.yaml"),
            package_copy.path().join("nested-child.yaml"),
        )
        .map_err(|error| error.to_string())?;

        let mut plan = runner_manifest::ManifestPlan::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("isolation-package.fixture.yaml"),
        )
        .map_err(|error| error.to_string())?;
        plan.package_root = package_copy.path().to_path_buf();
        plan.manifest.filesystem.read.clear();
        plan.manifest.filesystem.write.clear();
        plan.manifest.nested_contracts = vec!["nested-child.yaml".into()];
        let error = super::StagedTree::create(&plan)
            .err()
            .ok_or_else(|| "nested contract symlink escape unexpectedly staged".to_string())?;
        if !error.contains("nested contract manifest path traverses a symlink") {
            return Err(format!("unexpected nested staging error: {error}"));
        }

        let child = package_copy.path().join("child");
        fs::create_dir_all(&child).map_err(|error| error.to_string())?;
        fs::write(child.join("contract.yaml"), "version: '1'\n")
            .map_err(|error| error.to_string())?;
        let mut invalid_child_plan = plan;
        invalid_child_plan.manifest_path = package_copy.path().join("contract.yaml");
        invalid_child_plan.manifest.nested_contracts = vec!["child/contract.yaml".into()];
        let error = super::StagedTree::create(&invalid_child_plan)
            .err()
            .ok_or_else(|| "invalid nested contract manifest unexpectedly staged".to_string())?;
        if error.is_empty() {
            return Err("invalid nested contract manifest error was empty".into());
        }
    }

    Ok(())
}
