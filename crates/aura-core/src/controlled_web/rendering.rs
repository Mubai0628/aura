use super::*;

impl ControlledWeb {
    pub(super) async fn fetched_document(
        &self,
        page: &FetchedPage<'_>,
        mode: WebFetchMode,
        cancel: &mut watch::Receiver<bool>,
    ) -> OrCancel<RenderedDocument> {
        match mode {
            WebFetchMode::HttpOnly => OrCancel::Value(RenderedDocument {
                document: aura_web_runtime::extract_document(
                    page.url,
                    page.status,
                    page.content_type,
                    page.body,
                    mode,
                )
                .map_err(Into::into),
                stop_confirmed: true,
            }),
            WebFetchMode::Auto => {
                match aura_web_runtime::extract_document(
                    page.url,
                    page.status,
                    page.content_type,
                    page.body,
                    mode,
                ) {
                    Ok(document) => OrCancel::Value(RenderedDocument {
                        document: Ok(document),
                        stop_confirmed: true,
                    }),
                    Err(WebError::RenderingUnavailable) => {
                        self.render_fetched_document(page, true, cancel).await
                    }
                    Err(error) => OrCancel::Value(RenderedDocument {
                        document: Err(error.into()),
                        stop_confirmed: true,
                    }),
                }
            }
            WebFetchMode::HeadlessRenderRequired => {
                self.render_fetched_document(page, false, cancel).await
            }
        }
    }

    pub(super) async fn render_fetched_document(
        &self,
        page: &FetchedPage<'_>,
        auto_quality: bool,
        cancel: &mut watch::Receiver<bool>,
    ) -> OrCancel<RenderedDocument> {
        match aura_web_runtime::html_source(page.status, page.content_type, page.body) {
            Ok(source) => {
                let staged = match self.stage_render_page(page, &source, cancel).await {
                    OrCancel::Cancelled { stop_confirmed } => {
                        return OrCancel::Cancelled { stop_confirmed };
                    }
                    OrCancel::Value(Err(error)) => {
                        return OrCancel::Value(RenderedDocument {
                            document: Err(error),
                            stop_confirmed: true,
                        });
                    }
                    OrCancel::Value(Ok(page)) => page,
                };
                let session = match aura_windows_platform::start_headless_render_cancellable(
                    staged,
                    cancel.clone(),
                )
                .await
                {
                    Ok(None) => return OrCancel::Cancelled { stop_confirmed: true },
                    Ok(Some(session)) => session,
                    Err(error) => {
                        return OrCancel::Value(RenderedDocument {
                            document: Err(map_render_error(error).into()),
                            stop_confirmed: true,
                        });
                    }
                };
                let wait =
                    await_render_session(session, cancel.clone(), Duration::from_secs(25)).await;
                document_from_render_wait(wait, |rendered| {
                    let document = aura_web_runtime::document_from_rendered_html(
                        page.url,
                        &rendered.html,
                        ContentHash::sha256(page.body),
                    )?;
                    if auto_quality && document.text.chars().count() < 80 {
                        return Err(WebError::RenderingUnavailable.into());
                    }
                    Ok(document)
                })
            }
            Err(WebError::UnsupportedMediaType) => OrCancel::Value(RenderedDocument {
                document: aura_web_runtime::extract_document(
                    page.url,
                    page.status,
                    page.content_type,
                    page.body,
                    WebFetchMode::HttpOnly,
                )
                .map_err(Into::into),
                stop_confirmed: true,
            }),
            Err(error) => OrCancel::Value(RenderedDocument {
                document: Err(error.into()),
                stop_confirmed: true,
            }),
        }
    }

    pub(super) async fn stage_render_page(
        &self,
        page: &FetchedPage<'_>,
        html: &str,
        cancel: &mut watch::Receiver<bool>,
    ) -> OrCancel<Result<aura_windows_platform::HeadlessPage>> {
        let mut scripts = Vec::new();
        for resource in aura_web_runtime::executable_script_resources(page.url, html) {
            if resource.url.origin() != page.url.origin() {
                continue;
            }
            if scripts.len() >= 8 {
                return OrCancel::Value(Err(WebError::RenderingUnavailable.into()));
            }
            let bytes = match or_cancel(
                cancel,
                self.gateway_get(
                    page.dispatch,
                    &resource.url,
                    page.max_bytes,
                    "web.script_started",
                ),
            )
            .await
            {
                OrCancel::Cancelled { stop_confirmed } => {
                    return OrCancel::Cancelled { stop_confirmed };
                }
                OrCancel::Value(Err(error)) => return OrCancel::Value(Err(error)),
                OrCancel::Value(Ok((status, _, bytes))) => {
                    if !(200..300).contains(&status) {
                        return OrCancel::Value(Err(WebError::Http(status).into()));
                    }
                    if bytes.len() > 8 * 1024 * 1024 {
                        return OrCancel::Value(Err(WebError::ResponseTooLarge.into()));
                    }
                    bytes
                }
            };
            let index = scripts.len();
            scripts.push(aura_windows_platform::HeadlessScript {
                src: resource.src,
                file_name: format!("s{index}.js"),
                bytes,
            });
        }
        OrCancel::Value(Ok(aura_windows_platform::HeadlessPage { html: html.to_owned(), scripts }))
    }
}
