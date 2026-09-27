// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_code_mode::ImageDetail as CodeModeImageDetail;
use atlas_engine_protocol::models::DEFAULT_IMAGE_DETAIL;
use atlas_engine_protocol::models::FunctionCallOutputContentItem;
use atlas_engine_protocol::models::ImageDetail;

trait IntoProtocol<T> {
    fn into_protocol(self) -> T;
}

pub(super) fn into_function_call_output_content_items(
    items: Vec<atlas_engine_code_mode::FunctionCallOutputContentItem>,
) -> Vec<FunctionCallOutputContentItem> {
    items.into_iter().map(IntoProtocol::into_protocol).collect()
}

impl IntoProtocol<ImageDetail> for CodeModeImageDetail {
    fn into_protocol(self) -> ImageDetail {
        let value = self;
        match value {
            CodeModeImageDetail::Auto => ImageDetail::Auto,
            CodeModeImageDetail::Low => ImageDetail::Low,
            CodeModeImageDetail::High => ImageDetail::High,
            CodeModeImageDetail::Original => ImageDetail::Original,
        }
    }
}

impl IntoProtocol<FunctionCallOutputContentItem>
    for atlas_engine_code_mode::FunctionCallOutputContentItem
{
    fn into_protocol(self) -> FunctionCallOutputContentItem {
        let value = self;
        match value {
            atlas_engine_code_mode::FunctionCallOutputContentItem::InputText { text } => {
                FunctionCallOutputContentItem::InputText { text }
            }
            atlas_engine_code_mode::FunctionCallOutputContentItem::InputImage { image_url, detail } => {
                FunctionCallOutputContentItem::InputImage {
                    image_url,
                    detail: detail
                        .map(IntoProtocol::into_protocol)
                        .or(Some(DEFAULT_IMAGE_DETAIL)),
                }
            }
            atlas_engine_code_mode::FunctionCallOutputContentItem::InputAudio { audio_url } => {
                FunctionCallOutputContentItem::InputAudio { audio_url }
            }
        }
    }
}
