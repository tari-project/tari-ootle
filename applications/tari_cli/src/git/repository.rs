// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use git2::{build::RepoBuilder, Repository};
use thiserror::Error;

pub struct GitRepository {
    repository: Option<Repository>,
    local_folder: PathBuf,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Git2 error: {0}")]
    Git2(#[from] git2::Error),
    #[error("Git repository is not initialized!")]
    RepositoryNotInitialized,
    #[error("Invalid branch name!")]
    InvalidBranchName,
    #[error("Current reference is not a branch!")]
    RefIsNotBranch,
}

pub type GitRepositoryResult<T> = Result<T, Error>;

impl GitRepository {
    pub fn new(local_folder: PathBuf) -> Self {
        Self {
            repository: None,
            local_folder,
        }
    }

    /// Initializes a git repository in [`local_folder`].
    pub fn init(&mut self) -> GitRepositoryResult<()> {
        self.repository = Some(Repository::init(&self.local_folder)?);
        Ok(())
    }

    /// Loads an existing git repository from [`local_folder`].
    pub fn load(&mut self) -> GitRepositoryResult<()> {
        self.repository = Some(Repository::open(&self.local_folder).map_err(Error::Git2)?);

        Ok(())
    }

    /// Does a clone and checkout operation in [`local_folder`] based on the given repository [`url`] and [`branch`].
    pub fn clone_and_checkout(&mut self, url: &str, branch: &str) -> GitRepositoryResult<()> {
        self.repository = Some(
            RepoBuilder::new()
                .branch(branch)
                .clone(url, &self.local_folder)
                .map_err(Error::Git2)?,
        );

        Ok(())
    }

    /// Pulling latest changes on an optional branch (default is the current one).
    /// Note: this method always force checkout to latest head.
    pub fn pull_changes(&self, branch: Option<String>) -> GitRepositoryResult<()> {
        let repo = self.repository()?;
        let current_branch_name = if let Some(branch) = branch {
            branch
        } else {
            self.current_branch_name()?
        };
        let mut remote = repo.find_remote("origin")?;

        // fetch
        let mut fetch_opts = git2::FetchOptions::new();
        fetch_opts.download_tags(git2::AutotagOption::All);
        remote.fetch(&[current_branch_name.as_str()], Some(&mut fetch_opts), None)?;
        let fetch_head = repo.find_reference("FETCH_HEAD")?;
        let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

        // pull changes
        let refname = format!("refs/heads/{}", current_branch_name);
        repo.reference(
            &refname,
            fetch_commit.id(),
            true,
            &format!("Setting {} to {}", current_branch_name, fetch_commit.id()),
        )?;
        repo.set_head(&refname)?;
        repo.checkout_head(Some(
            git2::build::CheckoutBuilder::default()
                .allow_conflicts(false)
                .conflict_style_merge(true)
                .force(),
        ))?;

        Ok(())
    }

    /// Gives back the actual repository if initialized using any of the methods
    /// ([`Self::init`], [`Self::load`] or [`Self::clone_and_checkout`]).
    fn repository(&self) -> GitRepositoryResult<&Repository> {
        if self.repository.is_none() {
            return Err(Error::RepositoryNotInitialized);
        }

        Ok(self.repository.as_ref().unwrap())
    }

    /// Returns current branch name.
    pub fn current_branch_name(&self) -> GitRepositoryResult<String> {
        let repo = self.repository()?;
        let head = repo.head()?;
        if head.is_branch() {
            if let Some(name) = head.name() {
                Ok(name.to_string().replace("refs/heads/", ""))
            } else {
                Err(Error::InvalidBranchName)
            }
        } else {
            Err(Error::RefIsNotBranch)
        }
    }

    pub fn local_folder(&self) -> &PathBuf {
        &self.local_folder
    }
}
