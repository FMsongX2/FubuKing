// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_api::ApiError;
use atlas_engine_protocol::error::AtlasEngineErr;
use atlas_engine_protocol::error::AtlasEngineErrorDetails;
use http::StatusCode;

pub(super) const BEDROCK_EXPIRED_SIGNATURE_MESSAGE: &str = concat!(
    "Amazon Bedrock rejected the request because its AWS signature has expired. ",
    "Refresh your AWS credentials and retry. If `AWS_BEARER_TOKEN_BEDROCK` is set, ",
    "update or unset it, then restart Atlas Agent",
);

pub(super) fn map_api_error(error: ApiError) -> AtlasEngineErr {
    let error = atlas_engine_api::map_api_error(error);
    if let AtlasEngineErrorDetails::UnexpectedStatus(response) = error.details()
        && response.status == StatusCode::UNAUTHORIZED
        && response.body.contains("Signature expired:")
    {
        let mut response = response.clone();
        response.user_message = Some(BEDROCK_EXPIRED_SIGNATURE_MESSAGE.to_string());
        let mapped_error = AtlasEngineErr::new(AtlasEngineErrorDetails::UnexpectedStatus(response));
        return match error.retry_delay() {
            Some(retry_delay) => mapped_error.with_retry_delay(retry_delay),
            None => mapped_error,
        };
    }
    error
}
