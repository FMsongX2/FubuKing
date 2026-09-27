// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_config::CONFIG_TOML_FILE;
use atlas_engine_config::format_config_layer_source;
use atlas_engine_config::host_name;
use atlas_engine_config::loader::LocalTomlLayerStack;
use atlas_engine_config::loader::load_local_config_layers;
use atlas_engine_exec_server_protocol::EnvironmentConfigLayer;
use atlas_engine_exec_server_protocol::EnvironmentConfigLayerStack;
use atlas_engine_exec_server_protocol::EnvironmentConfigReadParams;
use atlas_engine_exec_server_protocol::EnvironmentConfigReadResponse;
use atlas_engine_file_system::ExecutorFileSystem;
use atlas_engine_utils_home_dir::find_atlas_agent_home;
use atlas_engine_utils_path_uri::PathUri;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ReadEnvironmentConfigError {
    #[error("{0}")]
    InvalidParams(String),
    #[error("{0}")]
    Internal(String),
}

pub(crate) async fn read_environment_config(
    file_system: &dyn ExecutorFileSystem,
    params: EnvironmentConfigReadParams,
) -> Result<EnvironmentConfigReadResponse, ReadEnvironmentConfigError> {
    validate_paths(&params)?;
    let cwd = params
        .cwd
        .to_abs_path()
        .map_err(|error| ReadEnvironmentConfigError::InvalidParams(error.to_string()))?;
    let atlas_agent_home = find_atlas_agent_home().map_err(|error| {
        ReadEnvironmentConfigError::Internal(format!("failed to find Atlas Agent home: {error}"))
    })?;
    let layers = load_local_config_layers(file_system, atlas_agent_home.as_path(), &cwd)
        .await
        .map_err(|error| {
            ReadEnvironmentConfigError::Internal(format!(
                "failed to load executor-local config: {error}"
            ))
        })?
        .project(&params.config_paths, &params.requirements_paths);

    Ok(EnvironmentConfigReadResponse {
        user_home_dir: dirs::home_dir()
            .and_then(|home_dir| PathUri::from_host_native_path(home_dir).ok()),
        atlas_agent_home_dir: PathUri::from_abs_path(&atlas_agent_home),
        hostname: host_name(),
        config: serialize_layer_stack(layers.config, |source| {
            format_config_layer_source(source, CONFIG_TOML_FILE)
        })?,
        requirements: serialize_layer_stack(layers.requirements, ToString::to_string)?,
    })
}

fn validate_paths(params: &EnvironmentConfigReadParams) -> Result<(), ReadEnvironmentConfigError> {
    if params.config_paths.is_empty() && params.requirements_paths.is_empty() {
        return Err(ReadEnvironmentConfigError::InvalidParams(
            "at least one config or requirements path is required".to_string(),
        ));
    }
    if params
        .config_paths
        .iter()
        .chain(&params.requirements_paths)
        .any(Vec::is_empty)
    {
        return Err(ReadEnvironmentConfigError::InvalidParams(
            "TOML paths must contain at least one key segment".to_string(),
        ));
    }
    Ok(())
}

fn serialize_layer_stack<S>(
    stack: LocalTomlLayerStack<S>,
    source_name: impl Fn(&S) -> String,
) -> Result<EnvironmentConfigLayerStack, ReadEnvironmentConfigError> {
    let layers = stack
        .layers
        .into_iter()
        .map(|layer| {
            let toml = toml::to_string(&layer.toml).map_err(|error| {
                ReadEnvironmentConfigError::Internal(format!(
                    "failed to serialize executor-local config: {error}"
                ))
            })?;
            Ok(EnvironmentConfigLayer {
                source: source_name(&layer.source),
                base_dir: PathUri::from_abs_path(&layer.base_dir),
                toml,
            })
        })
        .collect::<Result<Vec<_>, ReadEnvironmentConfigError>>()?;
    Ok(EnvironmentConfigLayerStack {
        layers,
        cloud_insertion_index: stack.cloud_insertion_index,
    })
}
