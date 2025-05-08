use eyre::{
    Context as _,
    Result,
};
use futures::StreamExt as _;
use playwright::{
    api::{
        Cookie,
        Page,
    },
    Playwright,
};
use std::path::PathBuf;
use url::Url;

fn get_binary() -> Result<PathBuf> {
    if let Ok(chromium) = which::which("chromium") {
        debug!(?chromium, "found chromium at");
        return Ok(chromium);
    } else {
        debug!("chromium not found, falling back to google-chrome");
    }

    let chrome = which::which("google-chrome").context("google chrome not found")?;
    debug!(?chrome, "found google-chrome at");
    Ok(chrome)
}

pub struct WebBrowser {
    _playwright: Playwright,
    browser: playwright::api::Browser,
    context: playwright::api::BrowserContext,
}

impl WebBrowser {
    pub async fn hyper_hyper(
        cookie: impl AsRef<str>,
        url: impl AsRef<str>,
        use_fake_media: bool,
        fake_video_file: Option<String>,
    ) -> Result<()> {
        let url = Url::parse(url.as_ref()).context("failed to parse url")?;
        let browser = Self::start_browser(&url, cookie, use_fake_media, fake_video_file).await?;
        let page = browser.visit_hyper(url).await?;
        browser.run_until_page_close(page).await?;
        Ok(())
    }

    pub async fn start_browser(
        url: &Url,
        cookie: impl AsRef<str>,
        use_fake_media: bool,
        fake_video_file: Option<String>,
    ) -> Result<Self, playwright::Error> {
        let playwright = Playwright::initialize().await?;

        // playwright.prepare()?; // Install browsers
        let binary = get_binary().expect("binary not found");

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
            .executable(&binary)
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

        let cookie = cookie.as_ref();
        if !cookie.is_empty() {
            let cookie = Cookie::with_domain_path("hyper_session", cookie, url.domain().unwrap_or(""), "/");
            context.add_cookies(&[cookie]).await?;
        }

        Ok(Self {
            _playwright: playwright,
            browser,
            context,
        })
    }

    #[expect(unused)]
    pub async fn close(self) -> Result<(), playwright::Error> {
        self.context.close().await?;
        self.browser.close().await?;
        Ok(())
    }

    pub async fn visit_hyper(&self, url: impl AsRef<str>) -> Result<Page, playwright::Error> {
        let page = self.context.new_page().await?;

        info!("page created");

        page.goto_builder(url.as_ref()).timeout(300_000.0).goto().await?;

        info!("joining...");
        page.click_builder(r#"button[type="submit"]"#)
            .timeout(300_000.0)
            .click()
            .await?;

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
                        Crash => trace!("browser event: Crash"),
                        Console(_console_message) => trace!("browser event: Console"),
                        Dialog => trace!("browser event: Dialog"),
                        DomContentLoaded => trace!("browser event: DomContentLoaded"),
                        Download(_download) => trace!("browser event: Download"),
                        FrameAttached(_frame) => trace!("browser event: FrameAttached"),
                        FrameDetached(_frame) => trace!("browser event: FrameDetached"),
                        FrameNavigated(_frame) => trace!("browser event: FrameNavigated"),
                        Load => trace!("browser event: Load"),
                        PageError => trace!("browser event: PageError"),
                        Popup(_page) => trace!("browser event: Popup"),
                        Request(_request) => trace!("browser event: Request"),
                        RequestFailed(_request) => trace!("browser event: RequestFailed"),
                        RequestFinished(_request) => trace!("browser event: RequestFinished"),
                        Response(_response) => trace!("browser event: Response"),
                        WebSocket(_web_socket) => trace!("browser event: WebSocket"),
                        Worker(_worker) => trace!("browser event: Worker"),
                        Video(_video) => trace!("browser event: Video"),
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
