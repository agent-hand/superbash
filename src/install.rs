//! only-install の mode（設計 2 章「only-install の mode」）。
//!
//! 生成した package（wrapper 無し）は置き場にある。その dir で `npm link` すると、
//! npm の global の prefix の `bin/` に bin の link ができ、`cmdA 引数`（global の bin が PATH にある時）と
//! `npx cmdA 引数` で動く。入れる command の範囲は生成の時に `--allow-cmd` で決まっている。

use anyhow::{Context, Result};

use crate::generate::Generated;
use crate::npm::Npm;

/// 生成した package の dir で npm link し、入った bin の名前を stderr に出す。
pub fn link(npm: &Npm, generated: &Generated) -> Result<()> {
    npm.link(&generated.package_dir)
        .with_context(|| format!("{} を npm link できない", generated.package_dir.display()))?;
    let names: Vec<&str> = generated.bins.keys().map(String::as_str).collect();
    if names.is_empty() {
        eprintln!("superbash: 入れる command は無い（--allow-cmd の範囲が空）");
    } else {
        eprintln!("superbash: npm link した: {}", names.join(", "));
    }
    Ok(())
}
