#![feature(min_specialization)]
#![feature(box_syntax)]

use anyhow::{anyhow, Result};

pub(crate) mod chunk;
pub(crate) mod parse;
pub(crate) mod transform;

use mdxjs::compile;
/*
use parse::{parse, ParseResult};
use swc_core::{
    common::{FilePathMapping, SourceMap, GLOBALS},
    ecma::{
        codegen::{text_writer::JsWriter, Emitter},
        visit::{VisitMutWith, VisitMutWithPath},
    },
}; */
use turbo_tasks::{primitives::StringVc, Value, ValueToString, ValueToStringVc};
use turbo_tasks_fs::{rope::Rope, File, FileContent, FileSystemPathVc};
use turbopack_core::{
    asset::{Asset, AssetContent, AssetContentVc, AssetVc},
    chunk::{ChunkItem, ChunkItemVc, ChunkVc, ChunkableAsset, ChunkableAssetVc, ChunkingContextVc},
    context::AssetContextVc,
    reference::AssetReferencesVc,
    resolve::origin::{ResolveOrigin, ResolveOriginVc},
    virtual_asset::VirtualAssetVc,
};
use turbopack_ecmascript::{
    chunk::{
        EcmascriptChunkItem, EcmascriptChunkItemContentVc, EcmascriptChunkItemVc,
        EcmascriptChunkPlaceable, EcmascriptChunkPlaceableVc, EcmascriptChunkVc,
    },
    AnalyzeEcmascriptModuleResultVc, EcmascriptInputTransformsVc, EcmascriptModuleAssetType,
    EcmascriptModuleAssetVc,
};

#[turbo_tasks::value]
#[derive(Clone, Copy)]
pub struct MdxModuleAsset {
    pub source: AssetVc,
    pub context: AssetContextVc,
    pub transforms: EcmascriptInputTransformsVc,
}

#[turbo_tasks::value_impl]
impl MdxModuleAssetVc {
    #[turbo_tasks::function]
    pub fn new(
        source: AssetVc,
        context: AssetContextVc,
        transforms: EcmascriptInputTransformsVc,
    ) -> Self {
        Self::cell(MdxModuleAsset {
            source,
            context,
            transforms,
        })
    }

    /// Perform analyze against mdx components.
    /// MDX components should be treated as normal j|tsx components to analyze
    /// its imports, only difference is it is not a valid ecmascript AST we
    /// can't pass it forward directly. Internally creates an jsx from mdx
    /// via mdxrs, then pass it through existing ecmascript analyzer.
    #[turbo_tasks::function]
    pub async fn analyze(self) -> Result<AnalyzeEcmascriptModuleResultVc> {
        let this = self.await?;
        let content = this.source.content();

        if let AssetContent::File(file) = &*content.await? {
            if let FileContent::Content(file) = &*file.await? {
                let file_conent = file.content().to_str()?;
                let mdx_jdx_component =
                    compile(&file_conent, &Default::default()).map_err(|e| anyhow!("{}", e))?;

                let result = Rope::from(mdx_jdx_component);
                let file = File::from(result);
                let source = VirtualAssetVc::new(this.source.path(), file.into());
                // alternatively, could try to use analyze_ecmascript_module directly
                let vc = EcmascriptModuleAssetVc::new(
                    source.into(),
                    this.context.into(),
                    Value::new(EcmascriptModuleAssetType::Typescript),
                    this.transforms,
                    this.context.environment(),
                );

                Ok(vc.analyze())
            } else {
                Err(anyhow!("Not able to read mdx file content"))
            }
        } else {
            Err(anyhow!("Unexpected mdx asset content"))
        }
    }

    /*
    pub async fn analyze(self) -> Result<AnalyzeEcmascriptModuleResultVc> {
        let this = self.await?;
        // Analyze import via `parsed` MDX input into swc ast -> str emitted
        let parsed = parse(this.source, this.transforms).await?;
        let p = this.source.path();

        match &*parsed {
            ParseResult::Ok { program, comments } => {
                if let Program::Module(mut module) = program.clone() {
                    let source = serialize(&mut module, None);
                    let result = Rope::from(source);
                    let file = File::from(result);
                    let source = VirtualAssetVc::new(p, file.into());

                    Ok(analyze_ecmascript_module(
                        source.into(),
                        self.as_resolve_origin(),
                        Value::new(EcmascriptModuleAssetType::Typescript),
                        this.transforms,
                        this.context.environment(),
                    ))
                } else {
                    anyhow::bail!("Parsed mdx cannot be non-module")
                }
            }
            _ => anyhow::bail!("parse error"),
        }
    } */
}

#[turbo_tasks::value_impl]
impl Asset for MdxModuleAsset {
    #[turbo_tasks::function]
    fn path(&self) -> FileSystemPathVc {
        self.source.path()
    }

    #[turbo_tasks::function]
    fn content(&self) -> AssetContentVc {
        self.source.content()
    }

    #[turbo_tasks::function]
    async fn references(self_vc: MdxModuleAssetVc) -> Result<AssetReferencesVc> {
        Ok(self_vc.analyze().await?.references)
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for MdxModuleAsset {
    #[turbo_tasks::function]
    fn as_chunk(self_vc: MdxModuleAssetVc, context: ChunkingContextVc) -> ChunkVc {
        EcmascriptChunkVc::new(context, self_vc.as_ecmascript_chunk_placeable()).into()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for MdxModuleAsset {
    #[turbo_tasks::function]
    fn as_chunk_item(
        self_vc: MdxModuleAssetVc,
        context: ChunkingContextVc,
    ) -> EcmascriptChunkItemVc {
        MdxChunkItemVc::cell(MdxChunkItem {
            module: self_vc,
            context,
        })
        .into()
    }
}

#[turbo_tasks::value_impl]
impl ResolveOrigin for MdxModuleAsset {
    #[turbo_tasks::function]
    fn origin_path(&self) -> FileSystemPathVc {
        self.source.path()
    }

    #[turbo_tasks::function]
    fn context(&self) -> AssetContextVc {
        self.context
    }
}

#[turbo_tasks::value]
struct MdxChunkItem {
    module: MdxModuleAssetVc,
    context: ChunkingContextVc,
}

#[turbo_tasks::value_impl]
impl ValueToString for MdxChunkItem {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        Ok(StringVc::cell(format!(
            "{} (mdx)",
            self.module.await?.source.path().to_string().await?
        )))
    }
}

#[turbo_tasks::value_impl]
impl ChunkItem for MdxChunkItem {
    #[turbo_tasks::function]
    fn references(&self) -> AssetReferencesVc {
        self.module.references()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for MdxChunkItem {
    #[turbo_tasks::function]
    fn chunking_context(&self) -> ChunkingContextVc {
        self.context
    }

    ///[TODOMdx]
    /// Once we have mdx contents, we should treat it as j|tsx components and
    /// apply all of the ecma transforms
    #[turbo_tasks::function]
    async fn content(&self) -> Result<EcmascriptChunkItemContentVc> {
        let this = self.module.await?;
        let content = this.source.content();

        if let AssetContent::File(file) = &*content.await? {
            if let FileContent::Content(file) = &*file.await? {
                let file_conent = file.content().to_str()?;
                let mdx_jdx_component =
                    compile(&file_conent, &Default::default()).map_err(|e| anyhow!("{}", e))?;

                let result = Rope::from(mdx_jdx_component);
                let file = File::from(result);
                let source = VirtualAssetVc::new(this.source.path(), file.into());
                let vc = EcmascriptModuleAssetVc::new(
                    source.into(),
                    this.context.into(),
                    Value::new(EcmascriptModuleAssetType::Typescript),
                    this.transforms,
                    this.context.environment(),
                );

                Ok(vc.as_chunk_item(self.context).content())
            } else {
                Err(anyhow!("Not able to read mdx file content"))
            }
        } else {
            Err(anyhow!("Unexpected mdx asset content"))
        }
    }

    /*
    async fn content(&self) -> Result<EcmascriptChunkItemContentVc> {
        let AnalyzeEcmascriptModuleResult {
            references,
            code_generation,
            ..
        } = &*self.module.analyze().await?;

        let context = self.context;
        let mut code_gens = Vec::new();
        for r in references.await?.iter() {
            if let Some(code_gen) = CodeGenerateableVc::resolve_from(r).await? {
                code_gens.push(code_gen.code_generation(context));
            }
        }
        for c in code_generation.await?.iter() {
            let c = c.resolve().await?;
            code_gens.push(c.code_generation(context));
        }
        // need to keep that around to allow references into that
        let code_gens = code_gens.into_iter().try_join().await?;
        let code_gens = code_gens.iter().map(|cg| &**cg).collect::<Vec<_>>();
        // TOOD use interval tree with references into "code_gens"
        let mut visitors = Vec::new();
        let mut root_visitors = Vec::new();
        for code_gen in code_gens {
            for (path, visitor) in code_gen.visitors.iter() {
                if path.is_empty() {
                    root_visitors.push(&**visitor);
                } else {
                    visitors.push((path, &**visitor));
                }
            }
        }

        let module = self.module.await?;
        let parsed = parse(module.source, module.transforms).await?;

        if let ParseResult::Ok { program, comments } = &*parsed {
            let mut program = program.clone();
            let source_map = Arc::new(SourceMap::new(FilePathMapping::empty()));

            GLOBALS.set(&Default::default(), || {
                if !visitors.is_empty() {
                    program.visit_mut_with_path(
                        &mut ApplyVisitors::new(visitors),
                        &mut Default::default(),
                    );
                }
                for visitor in root_visitors {
                    program.visit_mut_with(&mut visitor.create());
                }
                program.visit_mut_with(&mut swc_core::ecma::transforms::base::fixer::fixer(None));
            });

            let mut bytes: Vec<u8> = vec![];
            let mut srcmap = vec![];
            let mut emitter = Emitter {
                cfg: swc_core::ecma::codegen::Config {
                    ..Default::default()
                },
                cm: source_map.clone(),
                comments: None,
                wr: JsWriter::new(source_map.clone(), "\n", &mut bytes, Some(&mut srcmap)),
            };

            emitter.emit_program(&program)?;

            //[TODOMdx]
            //let srcmap = ParseResultSourceMap::new(source_map.clone(), srcmap).cell();

            Ok(EcmascriptChunkItemContent {
                inner_code: bytes.into(),
                source_map: None, //Some(srcmap),
                // [TodoMdx]
                options: EcmascriptChunkItemOptions {
                    // These things are not available in ESM
                    module: true,
                    exports: true,
                    this: true,
                    ..Default::default()
                },
                ..Default::default()
            }
            .into())
        } else {
            Ok(EcmascriptChunkItemContent {
                inner_code: format!(
                    "const e = new Error(\"Could not parse module '{path}'\");\ne.code = \
                     'MODULE_UNPARSEABLE';\nthrow e;",
                    path = self.module.path().to_string().await?
                )
                .into(),
                ..Default::default()
            }
            .into())
        }
    } */
}

pub fn register() {
    turbo_tasks::register();
    turbo_tasks_fs::register();
    turbopack_core::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}
