use std::{future::Future, sync::Arc};

use anyhow::Result;
use mdxjs::{interop_swc_ast, parse_hast_string, Location};
use swc_core::{
    base::SwcComments,
    common::{
        comments::Comments,
        errors::{Handler, HANDLER},
        Globals, Mark, SourceMap, GLOBALS,
    },
    ecma::{
        ast::Program,
        transforms::base::helpers::{Helpers, HELPERS},
    },
};
use turbo_tasks::ValueToString;
use turbo_tasks_fs::{FileContent, FileSystemPath};
use turbopack_core::asset::{AssetContent, AssetVc};
use turbopack_ecmascript::{
    hash_file_path, utils::WrapFuture, EcmascriptInputTransform, EcmascriptInputTransformsVc,
    TransformContext,
};
use turbopack_swc_utils::emitter::IssueEmitter;

#[turbo_tasks::value(shared, serialization = "none", eq = "manual")]
#[allow(clippy::large_enum_variant)]
pub enum ParseResult {
    Ok {
        #[turbo_tasks(trace_ignore)]
        program: Program,
        #[turbo_tasks(debug_ignore, trace_ignore)]
        comments: SwcComments,
        // [TODOMdx]
        //#[turbo_tasks(debug_ignore, trace_ignore)]
        //eval_context: EvalContext,
        //#[turbo_tasks(debug_ignore, trace_ignore)]
        //globals: Globals,
        //#[turbo_tasks(debug_ignore, trace_ignore)]
        //source_map: Arc<SourceMap>,
    },
    Unparseable,
    NotFound,
}

impl PartialEq for ParseResult {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Ok { .. }, Self::Ok { .. }) => false,
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

async fn parse_content(
    string: String,
    fs_path: &FileSystemPath,
    fs_path_str: &str,
    file_path_hash: u128,
    source: AssetVc,
    transforms: &[EcmascriptInputTransform],
) -> Result<ParseResultVc> {
    let source_map: Arc<SourceMap> = Default::default();
    let handler = Handler::with_emitter(
        true,
        false,
        box IssueEmitter {
            source,
            source_map: source_map.clone(),
            title: Some("Parsing mdx source code failed".to_string()),
        },
    );

    //[TODOMdx]
    let config = Default::default();

    //[TODOMdx]: Do we need `SourceFile`?
    let parsed_mdx = match parse_hast_string(&string, &config) {
        Ok(hast_node) => hast_node,
        Err(e) => {
            // [TODOMdx]: mdxrs currently returns untyped string as error, need to convert it for diagnostics.emit()
            println!("{:#?}", e);
            // TODO report in in a stream
            return Ok(ParseResult::Unparseable.into());
        }
    };

    let location = Location::new(string.as_bytes());
    // Note: this is not SWC's Program AST
    let program = match interop_swc_ast(&parsed_mdx, &location, &config) {
        Ok(program) => program,
        Err(e) => {
            // [TODOMdx]: mdxrs currently returns untyped string as error, need to convert it for diagnostics.emit()
            println!("{:#?}", e);
            // TODO report in in a stream
            return Ok(ParseResult::Unparseable.into());
        }
    };

    let globals = Globals::new();
    let globals_ref = &globals;
    let helpers = GLOBALS.set(globals_ref, || Helpers::new(true));

    let result = WrapFuture::new(
        |f, cx| {
            GLOBALS.set(globals_ref, || {
                HANDLER.set(&handler, || HELPERS.set(&helpers, || f.poll(cx)))
            })
        },
        async {
            let mut parsed_comments = program.comments;
            let mut program = swc_core::ecma::ast::Program::Module(program.module);

            let unresolved_mark = Mark::new();
            let top_level_mark = Mark::new();

            let comments = SwcComments::default();
            for c in parsed_comments.drain(..) {
                comments.add_leading(c.span.lo, c);
            }

            let context = TransformContext {
                comments: &comments,
                source_map: &source_map,
                top_level_mark,
                unresolved_mark,
                file_name_str: fs_path.file_name(),
                file_name_hash: file_path_hash,
            };

            for transform in transforms.iter() {
                transform.apply(&mut program, &context).await?;
            }

            Ok::<ParseResult, anyhow::Error>(ParseResult::Ok {
                program,
                comments,
                //eval_context,
                //globals: Globals::new(),
                //source_map,
            })
        },
    )
    .await?;

    /*if let ParseResult::Ok {
        globals: ref mut g, ..
    } = result
    {
        // Assign the correct globals
        *g = globals;
    }*/
    Ok(result.cell())
}

#[turbo_tasks::function]
pub async fn parse(
    source: AssetVc,
    transforms: EcmascriptInputTransformsVc,
) -> Result<ParseResultVc> {
    let content = source.content();
    let fs_path = &*source.path().await?;
    let fs_path_str = &*source.path().to_string().await?;
    let file_path_hash = *hash_file_path(source.path()).await? as u128;

    Ok(match &*content.await? {
        AssetContent::Redirect { .. } => ParseResult::Unparseable.cell(),
        AssetContent::File(file) => match &*file.await? {
            FileContent::NotFound => ParseResult::NotFound.cell(),
            FileContent::Content(file) => match file.content().to_str() {
                Err(_err) => ParseResult::Unparseable.cell(),
                Ok(string) => {
                    let transforms = &*transforms.await?;

                    parse_content(
                        string.into_owned(),
                        fs_path,
                        fs_path_str,
                        file_path_hash,
                        source,
                        transforms,
                    )
                    .await?
                }
            },
        },
    })
}
