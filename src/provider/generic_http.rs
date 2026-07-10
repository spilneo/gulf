use crate::{
    config::{EndpointConfig, ProviderConfig},
    model::QuoteRequest,
};
use anyhow::{Context, Result, bail};
use reqwest::Method;
use std::{collections::BTreeMap, str::FromStr, time::Duration};

use super::{
    EndpointCall, ProviderRegistry,
    error::redact_response_body,
    execution::{approval_token_address, normalize_execution_plan},
    response::{normalize_quote, normalize_transaction},
    template::{
        render_json_with_steps, render_optional_header, render_optional_param_with_steps,
        render_with_steps,
    },
};

impl ProviderRegistry {
    pub(super) async fn call_endpoint(
        &self,
        provider: &ProviderConfig,
        endpoint: Option<&EndpointConfig>,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        self.call_endpoint_with_steps(provider, endpoint, quote, &BTreeMap::new())
            .await
    }

    pub(super) async fn call_endpoint_with_steps(
        &self,
        provider: &ProviderConfig,
        endpoint: Option<&EndpointConfig>,
        quote: &QuoteRequest,
        step_outputs: &BTreeMap<String, serde_json::Value>,
    ) -> Result<EndpointCall> {
        let http = provider
            .http
            .as_ref()
            .with_context(|| format!("provider {} has no HTTP config", provider.id))?;
        let endpoint = endpoint
            .with_context(|| format!("provider {} endpoint is not configured", provider.id))?;
        let partner_fee = self.partner_fee(quote)?;
        let revenue = self.revenue_for_provider(provider, &partner_fee);
        if !self.provider_accepts_revenue(provider, revenue.integrator_fee_bps) {
            bail!(
                "provider {} does not allow the requested total integrator fee of {} bps",
                provider.id,
                revenue.integrator_fee_bps
            );
        }
        let rendered_path = render_with_steps(
            &endpoint.path,
            provider,
            quote,
            &self.config,
            &revenue,
            step_outputs,
        );
        let url = if rendered_path.starts_with("http://") || rendered_path.starts_with("https://") {
            rendered_path
        } else {
            format!("{}{}", http.base_url.trim_end_matches('/'), rendered_path)
        };
        let method = Method::from_str(&endpoint.method)?;
        let mut builder = self.client.request(method, url);

        if let Some(auth) = &http.auth {
            if let (Some(header), Some(value)) = (&auth.header, &auth.value)
                && let Some(rendered) =
                    render_optional_header(value, provider, quote, &self.config, &revenue)
            {
                builder = builder.header(header, rendered);
            }
            for (key, value) in &auth.headers {
                if let Some(rendered) =
                    render_optional_header(value, provider, quote, &self.config, &revenue)
                {
                    builder = builder.header(key, rendered);
                }
            }
        }

        for (key, value) in &http.default_headers {
            if let Some(rendered) =
                render_optional_header(value, provider, quote, &self.config, &revenue)
            {
                builder = builder.header(key, rendered);
            }
        }

        for (key, value) in &endpoint.headers {
            if let Some(rendered) =
                render_optional_header(value, provider, quote, &self.config, &revenue)
            {
                builder = builder.header(key, rendered);
            }
        }

        let mut query = http
            .default_query
            .iter()
            .filter_map(|(key, value)| {
                render_optional_param_with_steps(
                    value,
                    provider,
                    quote,
                    &self.config,
                    &revenue,
                    step_outputs,
                )
                .map(|rendered| (key.as_str(), rendered))
            })
            .collect::<Vec<_>>();
        if let Some(auth) = &http.auth {
            query.extend(auth.query.iter().filter_map(|(key, value)| {
                render_optional_param_with_steps(
                    value,
                    provider,
                    quote,
                    &self.config,
                    &revenue,
                    step_outputs,
                )
                .map(|rendered| (key.as_str(), rendered))
            }));
        }
        query.extend(endpoint.query.iter().filter_map(|(key, value)| {
            render_optional_param_with_steps(
                value,
                provider,
                quote,
                &self.config,
                &revenue,
                step_outputs,
            )
            .map(|rendered| (key.as_str(), rendered))
        }));
        if !query.is_empty() {
            builder = builder.query(&query);
        }

        if let Some(body) = &endpoint.body_json {
            builder = builder.json(&render_json_with_steps(
                body,
                provider,
                quote,
                &self.config,
                &revenue,
                step_outputs,
            ));
        }
        if !endpoint.body_form.is_empty() {
            let body = endpoint
                .body_form
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str(),
                        render_with_steps(
                            value,
                            provider,
                            quote,
                            &self.config,
                            &revenue,
                            step_outputs,
                        ),
                    )
                })
                .collect::<Vec<_>>();
            builder = builder.form(&body);
        }

        if let Some(timeout_ms) = endpoint.timeout_ms {
            builder = builder.timeout(Duration::from_millis(timeout_ms));
        }

        let request = builder.build()?;
        let attempts = endpoint.retry_attempts.unwrap_or(0).saturating_add(1);
        let mut last_error = None;
        for _ in 0..attempts {
            let request = request
                .try_clone()
                .context("request cannot be retried because its body is not cloneable")?;
            match self.client.execute(request).await {
                Ok(response) => {
                    let status = response.status();
                    let request_id = response
                        .headers()
                        .get("x-request-id")
                        .or_else(|| response.headers().get("cf-ray"))
                        .or_else(|| response.headers().get("x-amzn-requestid"))
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned);
                    let body = response.text().await?;
                    if !status.is_success() {
                        let request_id = request_id
                            .map(|id| format!(" request_id={id}"))
                            .unwrap_or_default();
                        bail!(
                            "provider {} HTTP {}{}: {}",
                            provider.id,
                            status.as_u16(),
                            request_id,
                            redact_response_body(&body)
                        );
                    }
                    let raw: serde_json::Value =
                        serde_json::from_str(&body).with_context(|| {
                            format!("provider {} returned non-JSON response", provider.id)
                        })?;
                    let mapping = endpoint.response.as_ref();
                    let token_addr = approval_token_address(provider, quote, &self.config);
                    return Ok(EndpointCall {
                        normalized_quote: mapping.map(|mapping| normalize_quote(mapping, &raw)),
                        normalized_transaction: mapping.map(|mapping| {
                            normalize_transaction(mapping, &raw, token_addr.clone())
                        }),
                        normalized_execution_plan: mapping.map(|mapping| {
                            normalize_execution_plan(
                                mapping,
                                &raw,
                                provider,
                                quote,
                                &self.config,
                                token_addr,
                            )
                        }),
                        raw,
                    });
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }
        }

        let error = last_error.context("request failed before it could be sent")?;
        Err(error.into())
    }

    pub(super) async fn call_prepare(
        &self,
        provider: &ProviderConfig,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        let Some(flow) = provider
            .http
            .as_ref()
            .and_then(|http| http.prepare_flow.as_ref())
        else {
            return self
                .call_endpoint(provider, self.endpoint(provider, "prepare"), quote)
                .await;
        };
        if flow.steps.is_empty() {
            bail!("provider {} prepare_flow has no steps", provider.id);
        }

        let mut step_outputs = BTreeMap::new();
        let mut step_order = Vec::new();
        let mut final_call = None;
        let mut final_endpoint = None;
        for step in &flow.steps {
            let endpoint = self.endpoint(provider, step);
            let call = self
                .call_endpoint_with_steps(provider, endpoint, quote, &step_outputs)
                .await
                .with_context(|| {
                    format!("provider {} prepare_flow step {step} failed", provider.id)
                })?;
            step_outputs.insert(step.clone(), call.raw.clone());
            step_order.push(step.clone());
            final_call = Some(call);
            final_endpoint = endpoint;
        }

        let mut final_call = final_call.context("prepare flow did not execute any steps")?;
        let mut steps = serde_json::Map::new();
        for step in step_order {
            if let Some(raw) = step_outputs.get(&step) {
                steps.insert(step, raw.clone());
            }
        }
        final_call.raw = serde_json::json!({
            "result": final_call.raw,
            "steps": steps,
        });
        if let Some(mapping) = final_endpoint.and_then(|endpoint| endpoint.response.as_ref()) {
            let token_addr = approval_token_address(provider, quote, &self.config);
            final_call.normalized_quote = Some(normalize_quote(mapping, &final_call.raw));
            final_call.normalized_transaction = Some(normalize_transaction(
                mapping,
                &final_call.raw,
                token_addr.clone(),
            ));
            final_call.normalized_execution_plan = Some(normalize_execution_plan(
                mapping,
                &final_call.raw,
                provider,
                quote,
                &self.config,
                token_addr,
            ));
        }
        Ok(final_call)
    }
}
