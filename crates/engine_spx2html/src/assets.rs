// Copyright 2022 the Tectonic Project
// Licensed under the MIT License.

//! Assets generated by a Tectonic HTML build.

use serde::Serialize;
use std::{collections::HashMap, fs::File, io::Write, path::Path};
use tectonic_errors::prelude::*;
use tectonic_status_base::tt_warning;

use crate::{fontfamily::FontEnsemble, specials::Special, Common};

#[derive(Debug, Default)]
pub(crate) struct Assets {
    paths: HashMap<String, AssetOrigin>,
}

#[derive(Debug)]
enum AssetOrigin {
    /// Copy a file from the source stack directly to the output directory.
    Copy(String),

    /// Emit a CSS file containing information about the ensemble of fonts
    /// that have been used.
    FontCss,
}

impl Assets {
    /// Returns true if the special was successfully handled. The false case
    /// doesn't distinguish between a special that wasn't relevant, and one that
    /// was malformatted or otherwise unparseable.
    pub fn try_handle_special(&mut self, special: Special, common: &mut Common) -> bool {
        match special {
            Special::ProvideFile(spec) => {
                let (src_tex_path, dest_path) = match spec.split_once(' ') {
                    Some(t) => t,
                    None => {
                        tt_warning!(common.status, "ignoring malformatted special `{}`", special);
                        return false;
                    }
                };

                self.copy_file(src_tex_path, dest_path);
                true
            }

            Special::ProvideSpecial(spec) => {
                let (kind, dest_path) = match spec.split_once(' ') {
                    Some(t) => t,
                    None => {
                        tt_warning!(common.status, "ignoring malformatted special `{}`", special);
                        return false;
                    }
                };

                match kind {
                    "font-css" => {
                        self.emit_font_css(dest_path);
                        true
                    }
                    _ => {
                        tt_warning!(common.status, "ignoring unsupported special `{}`", special);
                        false
                    }
                }
            }

            _ => false,
        }
    }

    fn copy_file<S1: ToString, S2: ToString>(&mut self, src_path: S1, dest_path: S2) {
        self.paths.insert(
            dest_path.to_string(),
            AssetOrigin::Copy(src_path.to_string()),
        );
    }

    fn emit_font_css<S: ToString>(&mut self, dest_path: S) {
        self.paths
            .insert(dest_path.to_string(), AssetOrigin::FontCss);
    }

    pub(crate) fn emit(mut self, mut fonts: FontEnsemble, common: &mut Common) -> Result<()> {
        let faces = fonts.emit(common.out_base)?;

        for (dest_path, origin) in self.paths.drain() {
            match origin {
                AssetOrigin::Copy(ref src_path) => emit_copied_file(src_path, &dest_path, common),
                AssetOrigin::FontCss => emit_font_css(&dest_path, &faces, common),
            }?;
        }

        Ok(())
    }

    pub(crate) fn into_serialize(mut self, fonts: FontEnsemble) -> impl Serialize {
        let (mut assets, css_data) = fonts.into_serialize();

        for (dest_path, origin) in self.paths.drain() {
            let info = match origin {
                AssetOrigin::Copy(src_path) => syntax::AssetOrigin::Copy(src_path),
                AssetOrigin::FontCss => syntax::AssetOrigin::FontCss(css_data.clone()),
            };
            assets.insert(dest_path, info);
        }

        assets
    }
}

fn emit_copied_file(src_tex_path: &str, dest_path: &str, common: &mut Common) -> Result<()> {
    // Set up input?

    let mut ih = atry!(
        common.hooks.io().input_open_name(src_tex_path, common.status).must_exist();
        ["unable to open provideFile source `{}`", &src_tex_path]
    );

    // Set up output? TODO: create parent directories!

    let mut out_path = common.out_base.to_owned();

    for piece in dest_path.split('/') {
        if piece.is_empty() {
            continue;
        }

        if piece == ".." {
            bail!(
                "illegal provideFile dest path `{}`: it contains a `..` component",
                &dest_path
            );
        }

        let as_path = Path::new(piece);

        if as_path.is_absolute() || as_path.has_root() {
            bail!(
                "illegal provideFile path `{}`: it contains an absolute/rooted component",
                &dest_path,
            );
        }

        out_path.push(piece);
    }

    // Copy!

    {
        let mut out_file = atry!(
            File::create(&out_path);
            ["cannot open output file `{}`", out_path.display()]
        );

        atry!(
            std::io::copy(&mut ih, &mut out_file);
            ["cannot copy to output file `{}`", out_path.display()]
        );
    }

    // All done.

    let (name, digest_opt) = ih.into_name_digest();
    common
        .hooks
        .event_input_closed(name, digest_opt, common.status);
    Ok(())
}

fn emit_font_css(dest_path: &str, faces: &str, common: &mut Common) -> Result<()> {
    let mut out_path = common.out_base.to_owned();

    for piece in dest_path.split('/') {
        if piece.is_empty() {
            continue;
        }

        if piece == ".." {
            bail!(
                "illegal provideFile dest path `{}`: it contains a `..` component",
                &dest_path
            );
        }

        let as_path = Path::new(piece);

        if as_path.is_absolute() || as_path.has_root() {
            bail!(
                "illegal provideFile path `{}`: it contains an absolute/rooted component",
                &dest_path,
            );
        }

        out_path.push(piece);
    }

    // Write

    let mut out_file = atry!(
        File::create(&out_path);
        ["cannot open output file `{}`", out_path.display()]
    );

    atry!(
        write!(&mut out_file, "{}", faces);
        ["cannot write output file `{}`", out_path.display()]
    );

    Ok(())
}

/// The concrete syntax for saving asset-output state, wired up via serde.
///
/// The top-level type is Assets.
pub(crate) mod syntax {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    use crate::fontfile::GlyphId;

    pub type Assets = HashMap<String, AssetOrigin>;

    #[derive(Debug, Deserialize, Serialize)]
    #[serde(tag = "kind")]
    pub enum AssetOrigin {
        /// Copy a file from the source stack directly to the output directory.
        Copy(String),

        /// Emit a CSS file containing information about the ensemble of fonts
        /// that have been used.
        FontCss(FontEnsembleAssetData),

        /// An OpenType/TrueType font file and variants with customized CMAP tables
        /// allowing access to unusual glyphs.
        FontFile(FontFileAssetData),
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    pub struct FontFileAssetData {
        /// The path to find the font file in the source stack.
        pub source: String,

        /// Variant glyphs that require us to emit alternate versions of the
        /// font file.
        pub vglyphs: HashMap<GlyphId, GlyphVariantMapping>,
    }

    #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
    pub struct GlyphVariantMapping {
        /// The USV that the glyph should be mapped to
        pub usv: char,

        /// Which alternative-mapped font to use. These indices start at zero.
        pub index: usize,
    }

    impl From<crate::fontfile::GlyphAlternateMapping> for GlyphVariantMapping {
        fn from(m: crate::fontfile::GlyphAlternateMapping) -> Self {
            GlyphVariantMapping {
                usv: m.usv,
                index: m.alternate_map_index,
            }
        }
    }

    /// Map from symbolic family name to info about the fonts defining it.
    pub type FontEnsembleAssetData = HashMap<String, FontFamilyAssetData>;

    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    pub struct FontFamilyAssetData {
        /// Map from face type to the output path of the font file providing it.
        pub faces: HashMap<FaceType, String>,
    }

    #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
    pub enum FaceType {
        /// The regular (upright) font of a font family.
        Regular,

        /// The bold font of a family.
        Bold,

        /// The italic font of a family.
        Italic,

        /// The bold-italic font a current family.
        BoldItalic,
    }
}
