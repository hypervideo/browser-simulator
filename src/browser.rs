use color_eyre::Result;
use futures::StreamExt as _;
use playwright::{
    api::{
        Cookie,
        Page,
    },
    Playwright,
};

pub struct WebBrowser {
    _playwright: Playwright,
    pub browser: playwright::api::Browser,
    pub context: playwright::api::BrowserContext,
}

impl WebBrowser {
    pub async fn hyper_hyper(
        cookie: impl AsRef<str>,
        url: impl AsRef<str>,
        use_fake_media: bool,
        fake_video_file: Option<String>,
    ) -> Result<(), playwright::Error> {
        let browser = Self::start_browser(cookie.as_ref(), use_fake_media, fake_video_file.clone()).await?;
        let page = browser.visit_hyper(url.as_ref()).await?;
        browser.run_until_page_close(page).await?;
        Ok(())
    }

    pub async fn start_browser(
        cookie: impl AsRef<str>,
        use_fake_media: bool,
        fake_video_file: Option<String>,
    ) -> Result<Self, playwright::Error> {
        let playwright = Playwright::initialize().await?;

        // playwright.prepare()?; // Install browsers
        let chromium = which::which("chromium").expect("chromium not found");
        debug!(?chromium, "found chromium at");

        let mut chrome_args = Vec::new();
        if use_fake_media {
            chrome_args.extend(
                ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream"]
                    .into_iter()
                    .map(String::from),
            );

            if let Some(path) = fake_video_file {
                chrome_args.push(format!("--use-file-for-fake-video-capture={path}"));
            }
        }

        let browser = playwright
            .chromium()
            .launcher()
            .args(&chrome_args) // Use the constructed args
            .executable(&chromium)
            .headless(false)
            .timeout(0.0)
            .launch()
            .await?;

        info!("browser launched");

        let context = browser
            .context_builder()
            .permissions(&[String::from("camera"), String::from("microphone")])
            .build()
            .await?;

        let cookie = Cookie::with_domain_path("hyper_session", cookie.as_ref(), "latest.dev.hyper.video", "/");
        context.add_cookies(&[cookie]).await?;

        Ok(Self {
            _playwright: playwright,
            browser,
            context,
        })
    }

    pub async fn close(self) -> Result<(), playwright::Error> {
        self.context.close().await?;
        self.browser.close().await?;
        Ok(())
    }

    pub async fn visit_hyper(&self, url: impl AsRef<str>) -> Result<Page, playwright::Error> {
        let page = self.context.new_page().await?;

        info!("page created");

        // page.goto_builder("https://latest.dev.hyper.video/KX7-0QQ-95Y")
        //     .goto()
        //     .await?;
        page.goto_builder(url.as_ref()).goto().await?;

        // info!("page loaded");
        // page.wait_for_timeout(5000.0).await;

        info!("joining...");
        page.click_builder(r#"button[type="submit"]"#).click().await?;

        Ok(page)
    }

    pub async fn run_until_page_close(self, page: Page) -> Result<(), playwright::Error> {
        let mut stream = page.subscribe_event()?;
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    use playwright::api::page::Event::*;
                    match event {
                        Close => break,
                        _ => {} /* Crash => todo!(),
                                 * Console(console_message) => todo!(),
                                 * Dialog => todo!(),
                                 * DomContentLoaded => todo!(),
                                 * Download(download) => todo!(),
                                 * FrameAttached(frame) => todo!(),
                                 * FrameDetached(frame) => todo!(),
                                 * FrameNavigated(frame) => todo!(),
                                 * Load => todo!(),
                                 * PageError => todo!(),
                                 * Popup(page) => todo!(),
                                 * Request(request) => todo!(),
                                 * RequestFailed(request) => todo!(),
                                 * RequestFinished(request) => todo!(),
                                 * Response(response) => todo!(),
                                 * WebSocket(web_socket) => todo!(),
                                 * Worker(worker) => todo!(),
                                 * Video(video) => todo!(), */
                    }
                }
                Err(e) => {
                    error!(?e);
                }
            }
        }
        Ok(())
    }
}
