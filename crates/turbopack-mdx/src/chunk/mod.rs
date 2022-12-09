#[turbo_tasks::value(shared)]
#[derive(Default)]
pub struct MdxChunkItemContent {
    //pub inner_code: Rope,
    //pub source_map: Option<ParseResultSourceMapVc>,
    //pub options: EcmascriptChunkItemOptions,
    pub placeholder_for_future_extensions: (),
}
