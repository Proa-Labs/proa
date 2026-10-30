use futures::executor::block_on;
use proa_core::{
    head_dispatch::{ComponentRole, HeadOperation},
    *,
};
use proa_stream::DataResolver;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Waker},
};
struct Product {
    name: String,
}
struct Loader(Arc<AtomicUsize>);
impl DataLoader for Loader {
    async fn load(&self, _r: &LoadRequest) -> Result<LoadValue, LoadError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(LoadValue::typed(Product {
            name: "product".into(),
        }))
    }
}
struct LoadedPage(Arc<Mutex<Option<Arc<Product>>>>);
impl<L: DataLoader> WebRender<L> for LoadedPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_head(&self, cx: &mut WebContext<L>) -> impl HeadOperation {
        async move {
            let p = cx
                .load_typed::<Product>(LoadKey::named("product", "1"))
                .await?;
            cx.head_mut().title(p.name.clone())?;
            *self.0.lock().unwrap() = Some(p);
            Ok(())
        }
    }
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let p = cx
                .load_typed::<Product>(LoadKey::named("product", "1"))
                .await?;
            assert!(Arc::ptr_eq(&p, self.0.lock().unwrap().as_ref().unwrap()));
            Ok(())
        })
    }
}
#[test]
fn head_and_body_share_materialized_value_but_fresh_requests_do_not() {
    let calls = Arc::new(AtomicUsize::new(0));
    let loader = Arc::new(Loader(calls.clone()));
    let mut products = Vec::new();
    for expected in 1..=2 {
        let resolver = DataResolver::request_scope(loader.clone());
        let mut cx = WebContext::with_loader(resolver);
        cx.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        let product = Arc::new(Mutex::new(None));
        block_on(render_child(&mut cx, LoadedPage(product.clone()))).unwrap();
        products.push(product.lock().unwrap().take().unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), expected);
    }
    assert!(!Arc::ptr_eq(&products[0], &products[1]));
}
struct DropCount(Arc<AtomicUsize>);
impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct PendingLoader(Arc<AtomicUsize>);
impl DataLoader for PendingLoader {
    async fn load(&self, _r: &LoadRequest) -> Result<LoadValue, LoadError> {
        let _drop = DropCount(self.0.clone());
        std::future::pending().await
    }
}
#[test]
fn abandoned_request_drops_pending_load_instead_of_retaining_a_cycle() {
    let drops = Arc::new(AtomicUsize::new(0));
    let resolver = DataResolver::request_scope(Arc::new(PendingLoader(drops.clone())));
    let req = LoadRequest::new(LoadKey::named("pending", "1"));
    {
        let mut load = std::pin::pin!(resolver.load(&req));
        assert!(load
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
    }
    assert_eq!(drops.load(Ordering::SeqCst), 0); // retained for other consumers in this request
    drop(resolver);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
