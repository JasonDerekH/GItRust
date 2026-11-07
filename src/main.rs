use std::env;
use std::io;
use std::process;

use git_rust::commits::{commit, head_commit, read_commit, Signature};
use git_rust::index::{add as add_to_index, read_index};
use git_rust::init;
use git_rust::object_store::{read_object, write_object};
use git_rust::paths::read_repository_file;
use git_rust::refs::{branch_reference, current_branch, list_refs, resolve_ref, update_ref};
use git_rust::trees::{read_tree, write_tree_from_index};
use git_rust::worktree::checkout;
use git_rust::worktrees::add as add_worktree;

fn main() {
    let mut arguments = env::args().skip(1);

    let Some(command) = arguments.next() else {
        eprintln!("usage: git-rust <command>");
        process::exit(1);
    };

    let result = match command.as_str() {
        "init" => init().map(|()| println!("Initialized git directory")),
        "add" => add_command(&mut arguments),
        "commit" => commit_command(&mut arguments),
        "hash-object" => hash_object(&mut arguments),
        "cat-file" => cat_file(&mut arguments),
        "write-tree" => write_tree_command(&mut arguments),
        "ls-tree" => ls_tree_command(&mut arguments),
        "log" => log_command(),
        "checkout" => checkout_command(&mut arguments),
        "branch" => branch_command(&mut arguments),
        "worktree" => worktree_command(&mut arguments),
        unknown => {
            eprintln!("unknown command: {unknown}");
            process::exit(1);
        }
    };

    if let Err(error) = result {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn cat_file(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    let first_argument = arguments.next();
    let object_id = match first_argument.as_deref() {
        Some("-p") => arguments.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "cat-file -p requires an object hash",
            )
        })?,
        Some(object_id) => object_id.to_owned(),
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cat-file requires an object hash",
            ));
        }
    };

    let object = read_object(&object_id)?;
    io::Write::write_all(&mut io::stdout(), &object.content)?;

    Ok(())
}

fn hash_object(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    let Some(file_name) = arguments.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "hash-object requires a file name",
        ));
    };

    let (_, content) = read_repository_file(file_name)?;
    let object_id = write_object("blob", &content)?;
    println!("{object_id}");

    Ok(())
}

fn write_tree_command(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    let _ = arguments;
    let entries = read_index()?;
    println!("{}", write_tree_from_index(&entries)?);
    Ok(())
}

fn add_command(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    let Some(path) = arguments.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "add requires a file or directory",
        ));
    };

    add_to_index(path)
}

fn commit_command(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    if arguments.next().as_deref() != Some("-m") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "commit requires -m followed by a message",
        ));
    }
    let Some(message) = arguments.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "commit requires a message",
        ));
    };

    let tree_id = write_tree_from_index(&read_index()?)?;
    let parents = head_commit()?.into_iter().collect::<Vec<_>>();
    let name = env::var("RUSTGIT_AUTHOR_NAME").unwrap_or_else(|_| "RustGit User".to_owned());
    let email =
        env::var("RUSTGIT_AUTHOR_EMAIL").unwrap_or_else(|_| "rustgit@example.com".to_owned());
    let author = Signature::now(name, email);
    let commit_id = commit(&tree_id, &parents, &author, &message)?;
    println!("{commit_id}");
    Ok(())
}

fn log_command() -> io::Result<()> {
    let mut current = head_commit()?;
    while let Some(commit_id) = current {
        let decoded = read_commit(&commit_id)?;
        println!("commit {commit_id}");
        println!("Author: {}", decoded.author);
        println!();
        println!("    {}", decoded.message.replace('\n', "\n    "));
        println!();
        current = decoded.parents.first().cloned();
    }
    Ok(())
}

fn checkout_command(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    let Some(target) = arguments.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "checkout requires a commit id or branch name",
        ));
    };

    checkout(&target)
}

fn branch_command(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    if let Some(name) = arguments.next() {
        let head = resolve_ref("HEAD")?;
        update_ref(&branch_reference(&name)?, &head)?;
        return Ok(());
    }

    let current = current_branch()?;
    for (reference, object_id) in list_refs()? {
        if let Some(name) = reference.strip_prefix("refs/heads/") {
            let marker = if current.as_deref() == Some(name) {
                "*"
            } else {
                " "
            };
            println!("{marker} {name} {object_id}");
        }
    }
    Ok(())
}

fn worktree_command(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    if arguments.next().as_deref() != Some("add") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "worktree requires the add subcommand",
        ));
    }
    let Some(path) = arguments.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "worktree add requires a path and branch",
        ));
    };
    let Some(branch) = arguments.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "worktree add requires a path and branch",
        ));
    };

    add_worktree(path, &branch)
}

fn ls_tree_command(arguments: &mut impl Iterator<Item = String>) -> io::Result<()> {
    let Some(tree_id) = arguments.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ls-tree requires a tree object id",
        ));
    };

    for entry in read_tree(&tree_id)? {
        let object_type = if entry.is_tree() { "tree" } else { "blob" };
        println!(
            "{:o} {} {}\t{}",
            entry.mode, object_type, entry.object_id, entry.name
        );
    }

    Ok(())
}
