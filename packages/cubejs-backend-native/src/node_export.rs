use cubesql::compile::parser::parse_sql_to_statement;
use cubesql::compile::{convert_statement_to_cube_query, get_df_batches};
use cubesql::config::processing_loop::ShutdownMode;
use cubesql::sql::dataframe::{arrow_to_column_type, Column};
use cubesql::sql::ColumnFlags;
use cubesql::transport::{SpanId, TransportService};
use futures::StreamExt;

use serde_json::Map;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::auth::{NativeSQLAuthContext, NodeBridgeAuthService, NodeBridgeAuthServiceOptions};
use crate::channel::call_js_fn;
use crate::config::{NodeConfiguration, NodeConfigurationFactoryOptions, NodeCubeServices};
use crate::cross::CLRepr;
use crate::cubesql_utils::with_session;
use crate::logger::NodeBridgeLogger;
use crate::sql4sql::sql4sql;
use crate::stream::OnDrainHandler;
use crate::tokio_runtime_node;
use crate::transport::NodeBridgeTransport;
use crate::utils::{batch_to_rows, NonDebugInRelease};
use cubenativeutils::wrappers::neon::context::neon_run_with_guarded_lifetime;
use cubenativeutils::wrappers::neon::inner_types::NeonInnerTypes;
use cubenativeutils::wrappers::neon::object::NeonObject;
use cubenativeutils::wrappers::object_handle::NativeObjectHandle;
use cubenativeutils::wrappers::serializer::NativeDeserialize;
use cubenativeutils::wrappers::NativeContextHolder;
use cubesqlplanner::cube_bridge::base_query_options::NativeBaseQueryOptions;
use cubesqlplanner::planner::base_query::BaseQuery;
use std::rc::Rc;
use std::sync::Arc;

use cubesql::telemetry::LocalReporter;
use cubesql::{telemetry::ReportingLogger, CubeError};
use neon::prelude::*;
use neon::result::Throw;
use simple_logger::SimpleLogger;

pub(crate) struct SQLInterface {
    pub(crate) services: Arc<NodeCubeServices>,
}

impl Finalize for SQLInterface {}

impl SQLInterface {
    pub fn new(services: Arc<NodeCubeServices>) -> Self {
        Self { services }
    }
}

fn get_function_from_options(
    options: Handle<JsObject>,
    name: &str,
    cx: &mut FunctionContext,
) -> Result<Root<JsFunction>, Throw> {
    let fun = options.get_opt::<JsFunction, _, _>(cx, name)?;
    if let Some(fun) = fun {
        Ok(fun.downcast_or_throw::<JsFunction, _>(cx)?.root(cx))
    } else {
        cx.throw_error(format!(
            "{} is required, must be passed as option in registerInterface",
            name
        ))
    }
}

fn register_interface<C: NodeConfiguration>(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let options = cx.argument::<JsObject>(0)?;

    let context_to_api_scopes = get_function_from_options(options, "contextToApiScopes", &mut cx)?;
    let check_auth = get_function_from_options(options, "checkAuth", &mut cx)?;
    let check_sql_auth = get_function_from_options(options, "checkSqlAuth", &mut cx)?;
    let transport_sql_api_load = get_function_from_options(options, "sqlApiLoad", &mut cx)?;
    let transport_sql = get_function_from_options(options, "sql", &mut cx)?;
    let transport_meta = get_function_from_options(options, "meta", &mut cx)?;
    let transport_log_load_event = get_function_from_options(options, "logLoadEvent", &mut cx)?;
    let transport_sql_generator = get_function_from_options(options, "sqlGenerators", &mut cx)?;
    let transport_can_switch_user_for_session =
        get_function_from_options(options, "canSwitchUserForSession", &mut cx)?;

    let pg_port_handle = options.get_value(&mut cx, "pgPort")?;
    let pg_port = if pg_port_handle.is_a::<JsNumber, _>(&mut cx) {
        let value = pg_port_handle.downcast_or_throw::<JsNumber, _>(&mut cx)?;

        Some(value.value(&mut cx) as u16)
    } else {
        None
    };

    let gateway_port = options.get_value(&mut cx, "gatewayPort")?;
    let gateway_port = if gateway_port.is_a::<JsNumber, _>(&mut cx) {
        let value = gateway_port.downcast_or_throw::<JsNumber, _>(&mut cx)?;

        Some(value.value(&mut cx) as u16)
    } else {
        None
    };

    let (deferred, promise) = cx.promise();
    let channel = cx.channel();

    let runtime = tokio_runtime_node(&mut cx)?;
    let transport_service = NodeBridgeTransport::new(
        cx.channel(),
        transport_sql_api_load,
        transport_sql,
        transport_meta,
        transport_log_load_event,
        transport_sql_generator,
        transport_can_switch_user_for_session,
    );
    let auth_service = NodeBridgeAuthService::new(
        cx.channel(),
        NodeBridgeAuthServiceOptions {
            check_auth,
            check_sql_auth,
            context_to_api_scopes,
        },
    );

    std::thread::spawn(move || {
        let config = C::new(NodeConfigurationFactoryOptions {
            gateway_port,
            pg_port,
        });

        runtime.block_on(async move {
            let services = config
                .configure(Arc::new(transport_service), Arc::new(auth_service))
                .await;

            let interface = SQLInterface::new(services.clone());

            log::debug!("Cube SQL Start");

            let mut loops = services.spawn_processing_loops().await.unwrap();
            loops.push(tokio::spawn(async move {
                deferred.settle_with(&channel, move |mut cx| Ok(cx.boxed(interface)));

                Ok(())
            }));
        });
    });

    Ok(promise)
}

fn shutdown_interface(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let interface = cx.argument::<JsBox<SQLInterface>>(0)?;
    let js_shutdown_mode = cx.argument::<JsString>(1)?;
    let shutdown_mode = match js_shutdown_mode.value(&mut cx).as_str() {
        "fast" => ShutdownMode::Fast,
        "semifast" => ShutdownMode::SemiFast,
        "smart" => ShutdownMode::Smart,
        _ => {
            return cx.throw_range_error::<&str, Handle<JsPromise>>(
                "ShutdownMode param must be 'fast', 'semifast', or 'smart'",
            );
        }
    };

    let (deferred, promise) = cx.promise();
    let channel = cx.channel();

    let services = interface.services.clone();
    let runtime = tokio_runtime_node(&mut cx)?;

    runtime.spawn(async move {
        match services.stop_processing_loops(shutdown_mode).await {
            Ok(_) => {
                if let Err(err) = services.await_processing_loops().await {
                    log::error!("Error during awaiting on shutdown: {}", err)
                }

                deferred
                    .settle_with(&channel, move |mut cx| Ok(cx.undefined()))
                    .await
                    .unwrap();
            }
            Err(err) => {
                channel.send(move |mut cx| {
                    let err = JsError::error(&mut cx, err.to_string()).unwrap();
                    deferred.reject(&mut cx, err);
                    Ok(())
                });
            }
        };
    });

    Ok(promise)
}

const CHUNK_DELIM: &str = "\n";

async fn write_jsonl_message(
    channel: Arc<Channel>,
    write_fn: Arc<Root<JsFunction>>,
    stream: Arc<Root<JsObject>>,
    value: serde_json::Value,
) -> Result<bool, CubeError> {
    let message = format!("{}{}", serde_json::to_string(&value)?, CHUNK_DELIM);

    call_js_fn(
        channel,
        write_fn,
        Box::new(move |cx| {
            let arg = cx.string(message).upcast::<JsValue>();
            Ok(vec![arg.upcast::<JsValue>()])
        }),
        Box::new(|cx, v| match v.downcast_or_throw::<JsBoolean, _>(cx) {
            Ok(v) => Ok(v.value(cx)),
            Err(_) => Err(CubeError::internal(
                "Failed to downcast write response".to_string(),
            )),
        }),
        stream,
    )
    .await
}

async fn handle_sql_query(
    services: Arc<NodeCubeServices>,
    native_auth_ctx: Arc<NativeSQLAuthContext>,
    channel: Arc<Channel>,
    stream_methods: WritableStreamMethods,
    sql_query: &str,
) -> Result<(), CubeError> {
    let span_id = Some(Arc::new(SpanId::new(
        Uuid::new_v4().to_string(),
        serde_json::json!({ "sql": sql_query }),
    )));

    let transport_service = services
        .injector()
        .get_service_typed::<dyn TransportService>()
        .await;

    with_session(&services, native_auth_ctx.clone(), |session| async move {
        if let Some(auth_context) = session.state.auth_context() {
            session
                .session_manager
                .server
                .transport
                .log_load_state(
                    span_id.clone(),
                    auth_context,
                    session.state.get_load_request_meta("sql"),
                    "Load Request".to_string(),
                    serde_json::json!({
                        "query": span_id.as_ref().unwrap().query_key,
                    }),
                )
                .await?;
        }

        let session_clone = Arc::clone(&session);
        let span_id_clone = span_id.clone();

        let execute = || async move {
            // todo: can we use compiler_cache?
            let meta_context = transport_service
                .meta(native_auth_ctx)
                .await
                .map_err(|err| {
                    CubeError::internal(format!("Failed to get meta context: {}", err))
                })?;

            let stmt =
                parse_sql_to_statement(sql_query, session.state.protocol.clone(), &mut None)?;
            let query_plan = convert_statement_to_cube_query(
                stmt,
                meta_context,
                session,
                &mut None,
                span_id_clone,
            )
            .await?;

            let mut stream = get_df_batches(&query_plan).await?;

            let semaphore = Arc::new(Semaphore::new(0));

            let drain_handler = OnDrainHandler::new(
                channel.clone(),
                stream_methods.stream.clone(),
                semaphore.clone(),
            );

            drain_handler.handle(stream_methods.on.clone()).await?;

            // Get schema from stream and convert to DataFrame columns format
            let stream_schema = stream.schema();
            let mut columns = Vec::with_capacity(stream_schema.fields().len());
            for field in stream_schema.fields().iter() {
                columns.push(Column::new(
                    field.name().clone(),
                    arrow_to_column_type(field.data_type().clone())?,
                    ColumnFlags::empty(),
                ));
            }

            // Send schema first
            let columns_json = serde_json::to_value(&columns)?;
            let mut schema_response = Map::new();
            schema_response.insert("schema".into(), columns_json);

            write_jsonl_message(
                channel.clone(),
                stream_methods.write.clone(),
                stream_methods.stream.clone(),
                serde_json::Value::Object(schema_response),
            )
            .await?;

            // Process all batches
            let mut has_data = false;
            while let Some(batch) = stream.next().await {
                let (_, data) = batch_to_rows(batch?)?;
                has_data = true;

                let mut rows = Map::new();
                rows.insert("data".into(), serde_json::Value::Array(data));

                let should_pause = !write_jsonl_message(
                    channel.clone(),
                    stream_methods.write.clone(),
                    stream_methods.stream.clone(),
                    serde_json::Value::Object(rows),
                )
                .await?;

                if should_pause {
                    let permit = semaphore.acquire().await?;
                    permit.forget();
                }
            }

            // If no data was processed, send empty data
            if !has_data {
                let mut rows = Map::new();
                rows.insert("data".into(), serde_json::Value::Array(vec![]));

                write_jsonl_message(
                    channel.clone(),
                    stream_methods.write.clone(),
                    stream_methods.stream.clone(),
                    serde_json::Value::Object(rows),
                )
                .await?;
            }

            Ok::<(), CubeError>(())
        };

        let result = execute().await;

        match &result {
            Ok(_) => {
                session_clone
                    .session_manager
                    .server
                    .transport
                    .log_load_state(
                        span_id.clone(),
                        session_clone.state.auth_context().unwrap(),
                        session_clone.state.get_load_request_meta("sql"),
                        "Load Request Success".to_string(),
                        serde_json::json!({
                            "query": {
                                "sql": sql_query,
                            },
                            "apiType": "sql",
                            "duration": span_id.as_ref().unwrap().duration(),
                            "isDataQuery": true
                        }),
                    )
                    .await?;
            }
            Err(err) => {
                session_clone
                    .session_manager
                    .server
                    .transport
                    .log_load_state(
                        span_id.clone(),
                        session_clone.state.auth_context().unwrap(),
                        session_clone.state.get_load_request_meta("sql"),
                        "Cube SQL Error".to_string(),
                        serde_json::json!({
                            "query": {
                                "sql": sql_query
                            },
                            "apiType": "sql",
                            "duration": span_id.as_ref().unwrap().duration(),
                            "error": err.message,
                        }),
                    )
                    .await?;
            }
        }

        result
    })
    .await
}

struct WritableStreamMethods {
    stream: Arc<Root<JsObject>>,
    on: Arc<Root<JsFunction>>,
    write: Arc<Root<JsFunction>>,
}

fn exec_sql(mut cx: FunctionContext) -> JsResult<JsValue> {
    let interface = cx.argument::<JsBox<SQLInterface>>(0)?;
    let sql_query = cx.argument::<JsString>(1)?.value(&mut cx);
    let node_stream = cx
        .argument::<JsObject>(2)?
        .downcast_or_throw::<JsObject, _>(&mut cx)?;

    let security_context: Option<serde_json::Value> = match cx.argument::<JsValue>(3) {
        Ok(string) => match string.downcast::<JsString, _>(&mut cx) {
            Ok(v) => v.value(&mut cx).parse::<serde_json::Value>().ok(),
            Err(_) => None,
        },
        Err(_) => None,
    };

    let js_stream_on_fn = Arc::new(
        node_stream
            .get::<JsFunction, _, _>(&mut cx, "on")?
            .root(&mut cx),
    );
    let js_stream_write_fn = Arc::new(
        node_stream
            .get::<JsFunction, _, _>(&mut cx, "write")?
            .root(&mut cx),
    );
    let js_stream_end_fn = Arc::new(
        node_stream
            .get::<JsFunction, _, _>(&mut cx, "end")?
            .root(&mut cx),
    );
    let node_stream_root = cx
        .argument::<JsObject>(2)?
        .downcast_or_throw::<JsObject, _>(&mut cx)?
        .root(&mut cx);

    let services = interface.services.clone();
    let runtime = tokio_runtime_node(&mut cx)?;

    let channel = Arc::new(cx.channel());
    let node_stream_arc = Arc::new(node_stream_root);

    let native_auth_ctx = Arc::new(NativeSQLAuthContext {
        user: Some(String::from("unknown")),
        superuser: false,
        security_context: NonDebugInRelease::from(security_context),
    });

    let (deferred, promise) = cx.promise();

    runtime.spawn(async move {
        let stream_methods = WritableStreamMethods {
            stream: node_stream_arc.clone(),
            on: js_stream_on_fn,
            write: js_stream_write_fn,
        };

        let result = handle_sql_query(
            services,
            native_auth_ctx,
            channel.clone(),
            stream_methods,
            &sql_query,
        )
        .await;

        let _ = channel.try_send(move |mut cx| {
            let method = match Arc::try_unwrap(js_stream_end_fn) {
                Ok(v) => v.into_inner(&mut cx),
                Err(v) => v.as_ref().to_inner(&mut cx),
            };
            let this = match Arc::try_unwrap(node_stream_arc) {
                Ok(v) => v.into_inner(&mut cx),
                Err(v) => v.as_ref().to_inner(&mut cx),
            };

            let args = match result {
                Ok(_) => vec![],
                Err(err) => {
                    let mut error_response = Map::new();
                    error_response.insert("error".into(), err.to_string().into());
                    let error_message = format!(
                        "{}{}",
                        serde_json::to_string(&serde_json::Value::Object(error_response))
                            .expect("Failed to serialize error response to JSON"),
                        CHUNK_DELIM
                    );
                    let arg = cx.string(error_message).upcast::<JsValue>();

                    vec![arg]
                }
            };

            method.call(&mut cx, this, args)?;

            Ok(())
        });

        deferred.settle_with(&channel, move |mut cx| Ok(cx.undefined()));
    });

    Ok(promise.upcast::<JsValue>())
}

fn is_fallback_build(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    #[cfg(feature = "python")]
    {
        return Ok(JsBoolean::new(&mut cx, false));
    }

    #[allow(unreachable_code)]
    Ok(JsBoolean::new(&mut cx, true))
}

fn get_log_level_from_variable(
    log_level_handle: Handle<JsValue>,
    cx: &mut FunctionContext,
) -> NeonResult<log::Level> {
    if log_level_handle.is_a::<JsString, _>(cx) {
        let value = log_level_handle.downcast_or_throw::<JsString, _>(cx)?;
        let log_level = match value.value(cx).as_str() {
            "error" => log::Level::Error,
            "warn" => log::Level::Warn,
            "info" => log::Level::Info,
            "debug" => log::Level::Debug,
            "trace" => log::Level::Trace,
            x => cx.throw_error(format!("Unrecognized log level: {}", x))?,
        };

        Ok(log_level)
    } else {
        Ok(log::Level::Trace)
    }
}

pub fn setup_logger(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let options = cx.argument::<JsObject>(0)?;
    let cube_logger = options
        .get::<JsFunction, _, _>(&mut cx, "logger")?
        .root(&mut cx);

    let log_level_handle = options.get_value(&mut cx, "logLevel")?;
    let log_level = get_log_level_from_variable(log_level_handle, &mut cx)?;

    let logger = create_logger(log_level);
    log_reroute::reroute_boxed(Box::new(logger));

    ReportingLogger::init(
        Box::new(NodeBridgeLogger::new(cx.channel(), cube_logger)),
        log_level.to_level_filter(),
    )
    .unwrap();

    Ok(cx.undefined())
}

pub fn create_logger(log_level: log::Level) -> SimpleLogger {
    SimpleLogger::new()
        .with_level(log::Level::Error.to_level_filter())
        .with_module_level("cubesql", log_level.to_level_filter())
        .with_module_level("cube_xmla", log_level.to_level_filter())
        .with_module_level("cube_xmla_engine", log_level.to_level_filter())
        .with_module_level("cubejs_native", log_level.to_level_filter())
        .with_module_level("datafusion", log::Level::Warn.to_level_filter())
        .with_module_level("pg_srv", log::Level::Warn.to_level_filter())
}

pub fn setup_local_logger(log_level: log::Level) {
    let logger = create_logger(log_level);
    log_reroute::reroute_boxed(Box::new(logger));

    ReportingLogger::init(
        Box::new(LocalReporter::new()),
        log::Level::Error.to_level_filter(),
    )
    .unwrap();
}

pub fn reset_logger(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let options = cx.argument::<JsObject>(0)?;

    let log_level_handle = options.get_value(&mut cx, "logLevel")?;
    let log_level = get_log_level_from_variable(log_level_handle, &mut cx)?;

    setup_local_logger(log_level);

    Ok(cx.undefined())
}

//============ sql planner ===================

fn build_sql_and_params(cx: FunctionContext) -> JsResult<JsValue> {
    neon_run_with_guarded_lifetime(cx, |neon_context_holder| {
        let options =
            NativeObjectHandle::<NeonInnerTypes<FunctionContext<'static>>>::new(NeonObject::new(
                neon_context_holder.clone(),
                neon_context_holder
                    .with_context(|cx| cx.argument::<JsValue>(0))
                    .unwrap()?,
            ));

        let safe_call_fn = neon_context_holder
            .with_context(|cx| {
                if let Ok(func) = cx.argument::<JsFunction>(1) {
                    Some(func)
                } else {
                    None
                }
            })
            .unwrap();

        neon_context_holder.set_safe_call_fn(safe_call_fn).unwrap();

        let context_holder = NativeContextHolder::<NeonInnerTypes<FunctionContext<'static>>>::new(
            neon_context_holder,
        );

        let base_query_options = Rc::new(NativeBaseQueryOptions::from_native(options).unwrap());

        let base_query = BaseQuery::try_new(context_holder.clone(), base_query_options).unwrap();

        let res = base_query.build_sql_and_params();

        let result: NeonObject<FunctionContext<'static>> = res.into_object();
        let result = result.into_object();
        Ok(result)
    })
}

fn debug_js_to_clrepr_to_js(mut cx: FunctionContext) -> JsResult<JsValue> {
    let arg = cx.argument::<JsValue>(0)?;
    let arg_clrep = CLRepr::from_js_ref(arg, &mut cx)?;

    arg_clrep.into_js(&mut cx)
}

pub fn register_module_exports<C: NodeConfiguration + 'static>(
    mut cx: ModuleContext,
) -> NeonResult<()> {
    cx.export_function("setupLogger", setup_logger)?;
    cx.export_function("resetLogger", reset_logger)?;
    cx.export_function("registerInterface", register_interface::<C>)?;
    cx.export_function("shutdownInterface", shutdown_interface)?;
    cx.export_function("execSql", exec_sql)?;
    cx.export_function("sql4sql", sql4sql)?;
    cx.export_function("isFallbackBuild", is_fallback_build)?;
    cx.export_function("__js_to_clrepr_to_js", debug_js_to_clrepr_to_js)?;

    //============ sql planner exports ===================
    cx.export_function("buildSqlAndParams", build_sql_and_params)?;

    //========= sql orchestrator exports =================
    crate::orchestrator::register_module(&mut cx)?;

    crate::template::template_register_module(&mut cx)?;

    #[cfg(feature = "python")]
    crate::python::python_register_module(&mut cx)?;

    Ok(())
}
