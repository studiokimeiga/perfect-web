// Included by uploads::tests. This module exercises the real upload/lease
// operations with accounts enabled; the body boundary uses an owned channel,
// without sleeps or process-global identity changes.
mod session_authority_tests {
    use super::*;
    use crate::identity::{Identity, Principals, ReaderIdentity};
    use std::net::{Shutdown, TcpListener};
    use std::sync::mpsc;

    const COOKIE: &str = "private-upload-authority";

    fn accounts(u: &Uploads) -> (Identity, Principals) {
        let identity = Identity::default();
        identity.signed_in_for_test(COOKIE, "alice", "Alice");
        let principals = identity.principals();
        u.identified_by(principals.clone());
        (identity, principals)
    }

    // Exercise the real completed logout path and establish its live result
    // before asking an old cookie to read or change an upload.
    fn logout(identity: &Identity) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
        let mut client =
            TcpStream::connect(listener.local_addr().expect("address")).expect("client");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let (mut server, _) = listener.accept().expect("server");
        assert!(identity.answer(
            "POST", "/sign-out",
            "Host: 127.0.0.1:3143\r\nOrigin: http://127.0.0.1:3143\r\nSec-Fetch-Site: same-origin\r\n",
            COOKIE, false, &[], &mut server, "", &|_| {},
        ));
        server.shutdown(Shutdown::Write).expect("finished response");
        let mut reply = String::new();
        client.read_to_string(&mut reply).expect("logout response");
        assert!(reply.starts_with("HTTP/1.1 303"), "logout must complete");
        assert_eq!(
            identity.principals().inspect_reader(COOKIE),
            Ok(ReaderIdentity::Anonymous)
        );
    }

    fn attached(u: &Uploads, principals: &Principals) -> (ReaderIdentity, Shown) {
        let reader = principals.bound_reader(COOKIE);
        principals.with_reader(COOKIE, &reader, || {
            assert_eq!(
                post(u, COOKIE, &png(9, 7), "private.png", "image/png").status,
                303
            );
            let shown = shared(u).shown(COOKIE).required("current reader's lease");
            let preview = get(u, &shown.src, COOKIE, "");
            assert_eq!(preview.status, 200, "current reader may preview");
            assert_eq!(preview.body, png(9, 7), "preview is the staged image");
            (reader.clone(), shown)
        })
    }

    #[test]
    fn current_reader_can_preview_claim_rollback_and_publish_public_image() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (identity, principals) = accounts(&u);
        let (reader, shown) = attached(&u, &principals);
        principals.with_reader(COOKIE, &reader, || {
            let claims = Claims::new(u.leases());
            let result =
                claim_op(claims.clone())(&[Val::String(COOKIE.into())]).required("claim call");
            assert!(
                matches!(result.as_slice(), [Val::Result(Ok(_))]),
                "current reader may claim"
            );
            drop(claims);
            assert!(
                shared(&u).shown(COOKIE).is_some(),
                "rollback returns the same reader's lease"
            );
            let claimed = shared(&u).claim(COOKIE).required("claim after rollback");
            shared(&u).keep(&claimed).required("blob preparation");
            let key = claimed.key.clone();
            let kind = claimed.kind;
            shared(&u).committed(claimed);
            let named_key = key.hex().to_string();
            u.named_by(Box::new(move |key| Ok(key == named_key)));
            assert_eq!(
                get(&u, &shown.src, COOKIE, "").status,
                404,
                "committed lease ended"
            );
            logout(&identity);
            let public = get(&u, &shared(&u).src(&key, kind), "any-reader", "");
            assert_eq!(public.status, 200, "a committed named image remains public");
            assert_eq!(public.body, png(9, 7));
        });
    }

    #[test]
    fn completed_logout_refuses_old_cookie_preview_shown_and_claim() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (identity, principals) = accounts(&u);
        let (_, shown) = attached(&u, &principals);
        logout(&identity);
        let anonymous = principals.bound_reader(COOKIE);
        principals.with_reader(COOKIE, &anonymous, || {
            assert_eq!(
                get(&u, &shown.src, COOKIE, "").status,
                404,
                "logout must hide the private preview"
            );
            assert!(
                shared(&u).shown(COOKIE).is_none(),
                "logout must hide the attachment metadata"
            );
            assert!(
                shared(&u).claim(COOKIE).is_none(),
                "logout must not acquire a predecessor's lease"
            );
        });
    }

    #[test]
    fn same_cookie_same_account_new_generation_cannot_preview_or_claim_old_lease() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (identity, principals) = accounts(&u);
        let (old, shown) = attached(&u, &principals);
        identity.signed_in_for_test(COOKIE, "alice", "Alice");
        let replacement = principals.bound_reader(COOKIE);
        assert_ne!(old, replacement, "the same actor has a fresh generation");
        principals.with_reader(COOKIE, &replacement, || {
            assert_eq!(
                get(&u, &shown.src, COOKIE, "").status,
                404,
                "a new generation cannot inherit private bytes"
            );
            assert!(
                shared(&u).shown(COOKIE).is_none(),
                "a new generation cannot inherit metadata"
            );
            assert!(
                shared(&u).claim(COOKIE).is_none(),
                "a new generation cannot acquire the old image"
            );
            assert_eq!(
                post(&u, COOKIE, &png(3, 5), "new.png", "image/png").status,
                303,
                "new authority can upload its own image"
            );
            let own = shared(&u).shown(COOKIE).required("new generation's lease");
            assert_eq!(get(&u, &own.src, COOKIE, "").body, png(3, 5));
        });
    }

    #[test]
    fn stale_bound_reader_cannot_take_replacement_generations_lease() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (identity, principals) = accounts(&u);
        let old = principals.bound_reader(COOKIE);
        identity.signed_in_for_test(COOKIE, "alice", "Alice");
        let (_, current) = attached(&u, &principals);
        principals.with_reader(COOKIE, &old, || {
            assert_eq!(
                get(&u, &current.src, COOKIE, "").status,
                404,
                "stale request cannot read replacement's image"
            );
            assert!(
                shared(&u).shown(COOKIE).is_none(),
                "bound reader is not recaptured from the live cache"
            );
            assert!(
                shared(&u).claim(COOKIE).is_none(),
                "stale command cannot claim replacement's image"
            );
        });
    }

    #[test]
    fn original_bound_reader_cannot_preview_after_completed_logout() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (identity, principals) = accounts(&u);
        let (original, shown) = attached(&u, &principals);
        principals.with_reader(COOKIE, &original, || {
            logout(&identity);
            assert_eq!(
                get(&u, &shown.src, COOKIE, "").status,
                404,
                "preview must inspect live authority even when the lease matches its original bound reader"
            );
        });
    }

    struct RemoteSource {
        identity: Mutex<Option<crate::identity::SessionIdentity>>,
        unavailable: std::sync::atomic::AtomicBool,
        admissions: std::sync::atomic::AtomicUsize,
        after_inspect: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl crate::identity::Kept for RemoteSource {
        fn session(
            &self,
            hash: &[u8],
            life: &crate::identity::SessionLifetimes,
        ) -> Result<Option<crate::identity::SessionIdentity>, String> {
            self.admissions
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inspect(hash, life)
        }
        fn inspect(
            &self,
            _: &[u8],
            _: &crate::identity::SessionLifetimes,
        ) -> Result<Option<crate::identity::SessionIdentity>, String> {
            let after = self.after_inspect.lock().expect("source hook").take();
            if let Some(after) = after {
                after();
            }
            if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
                Err("PRIVATE_UPLOAD_AUTHORITY_SOURCE_FAILURE".into())
            } else {
                Ok(self.identity.lock().expect("remote authority").clone())
            }
        }
        fn open(
            &self,
            _: &[u8],
            principal: &crate::identity::Principal,
            epoch: &[u8; 16],
        ) -> Result<(), String> {
            *self.identity.lock().expect("remote authority") =
                Some(crate::identity::SessionIdentity {
                    principal: principal.clone(),
                    epoch: *epoch,
                });
            Ok(())
        }
        fn close(&self, _: &[u8]) -> Result<(), String> {
            *self.identity.lock().expect("remote authority") = None;
            Ok(())
        }
        fn begin_sign_in(&self, _: &[u8], _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn take_sign_in(&self, _: &[u8]) -> Result<Option<(String, String, Duration)>, String> {
            Ok(None)
        }
    }

    fn remote_source(principals: &Principals) -> Arc<RemoteSource> {
        let ReaderIdentity::Account(identity) = principals.bound_reader(COOKIE) else {
            panic!("account fixture");
        };
        let source = Arc::new(RemoteSource {
            identity: Mutex::new(Some(identity)),
            unavailable: std::sync::atomic::AtomicBool::new(false),
            admissions: std::sync::atomic::AtomicUsize::new(0),
            after_inspect: Mutex::new(None),
        });
        principals.keep_in(source.clone());
        source
    }

    #[test]
    fn unavailable_source_cannot_serve_preview_from_cached_bound_reader() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (_, principals) = accounts(&u);
        let source = remote_source(&principals);
        let (reader, shown) = attached(&u, &principals);
        source
            .unavailable
            .store(true, std::sync::atomic::Ordering::SeqCst);
        principals.with_reader(COOKIE, &reader, || {
            let refused = get(&u, &shown.src, COOKIE, "");
            assert_ne!(
                refused.status, 200,
                "unavailable source must refuse preview instead of borrowing cached authority"
            );
            assert_ne!(
                refused.body,
                png(9, 7),
                "source failure exposes no private preview bytes"
            );
            assert!(
                !body(&refused).contains("PRIVATE_UPLOAD_AUTHORITY_SOURCE_FAILURE"),
                "authority source diagnostics are bounded"
            );
        });
        source
            .unavailable
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let recovered = get(&u, &shown.src, COOKIE, "");
        assert_eq!(
            recovered.status, 200,
            "source recovery preserves the rightful lease"
        );
        assert_eq!(recovered.body, png(9, 7));
        assert_eq!(
            source.admissions.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "background inspection never renews client activity"
        );
    }

    #[test]
    fn remote_revocation_without_local_refresh_cannot_serve_preview() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (_, principals) = accounts(&u);
        let source = remote_source(&principals);
        let (reader, shown) = attached(&u, &principals);
        let saved = source.identity.lock().expect("remote authority").take();
        assert_eq!(
            principals.bound_reader(COOKIE),
            reader,
            "this host still caches the old generation"
        );
        principals.with_reader(COOKIE, &reader, || {
            assert_eq!(
                get(&u, &shown.src, COOKIE, "").status,
                404,
                "source revocation must hide preview even before this host refreshes"
            );
        });
        assert_eq!(
            source.admissions.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "source inspection must not touch client admission"
        );
        *source.identity.lock().expect("remote authority") = saved;
        assert_eq!(
            get(&u, &shown.src, COOKIE, "").status,
            200,
            "unchanged original source is the positive neighbor"
        );
    }

    fn a_source_change_in_body_refuses_staging(unavailable: bool) {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (_, principals) = accounts(&u);
        let source = remote_source(&principals);
        // Identical upload has a current-source success neighbor.
        let _ = attached(&u, &principals);
        shared(&u).discarded(shared(&u).claim(COOKIE).required("clear neighbor"));
        let original = principals.bound_reader(COOKIE);
        let (ct, body) = form(&png(11, 6), "remote-held.png", "image/png");
        let headers = headers_for(&ct, body.len());
        let (reached_tx, reached_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let u = &u;
            let principals = &principals;
            let reader = &original;
            let worker = scope.spawn(move || {
                let mut body = HeldBody {
                    bytes: Cursor::new(body),
                    reached: Some(reached_tx),
                    release: release_rx,
                };
                principals.with_reader(COOKIE, reader, || {
                    shared(u).answer(
                        "POST",
                        "/uploads/post-image",
                        &headers,
                        COOKIE,
                        false,
                        &mut body,
                    )
                })
            });
            reached_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("body boundary");
            if unavailable {
                source
                    .unavailable
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            } else {
                *source.identity.lock().expect("remote authority") = None;
            }
            assert_eq!(
                principals.bound_reader(COOKIE),
                original,
                "local cached authority has not refreshed"
            );
            release_tx.send(()).expect("release owned reader");
            let answer = worker.join().expect("upload worker");
            assert_ne!(
                answer.status, 303,
                "post-body source change must refuse lease publication despite a matching local cache"
            );
            assert!(
                !super::body(&answer).contains("PRIVATE_UPLOAD_AUTHORITY_SOURCE_FAILURE"),
                "post-body authority failures expose no private backend payload"
            );
        });
        let state = shared(&u).state.lock().expect("state");
        assert!(
            state.leases.is_empty(),
            "source-refused upload publishes no lease"
        );
        assert_eq!(
            state.staged_bytes, 0,
            "source-refused upload retains no staging"
        );
        drop(state);
        assert_eq!(files(&d.path().join("staged")), 0);
        assert_eq!(
            source.admissions.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "final publication inspection does not touch idle activity"
        );
    }

    #[test]
    fn remote_revocation_while_upload_body_is_held_refuses_staging() {
        a_source_change_in_body_refuses_staging(false);
    }

    #[test]
    fn source_failure_while_upload_body_is_held_refuses_staging() {
        a_source_change_in_body_refuses_staging(true);
    }

    #[derive(Default)]
    struct BlobProbe {
        bytes: MemoryBlobs,
        after_put: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        after_get: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    struct ProbedBlobs(Arc<BlobProbe>);

    impl BlobStore for ProbedBlobs {
        fn put(&self, bytes: &[u8]) -> std::io::Result<BlobKey> {
            let key = self.0.bytes.put(bytes)?;
            let after = self.0.after_put.lock().expect("put hook").take();
            if let Some(after) = after {
                after();
            }
            Ok(key)
        }
        fn get(&self, key: &BlobKey) -> std::io::Result<Option<Vec<u8>>> {
            let bytes = self.0.bytes.get(key)?;
            let after = self.0.after_get.lock().expect("get hook").take();
            if let Some(after) = after {
                after();
            }
            Ok(bytes)
        }
        fn delete(&self, key: &BlobKey) -> std::io::Result<()> {
            self.0.bytes.delete(key)
        }
        fn count(&self) -> std::io::Result<usize> {
            self.0.bytes.count()
        }
    }

    fn probed_uploads() -> (Uploads, Arc<BlobProbe>) {
        let probe = Arc::new(BlobProbe::default());
        let u = Uploads::with(
            vec![declared()],
            Box::new(ProbedBlobs(probe.clone())),
            Box::new(MemoryBlobs::default()),
            LEASE,
        );
        (u, probe)
    }

    #[test]
    fn local_expiry_after_staged_put_refuses_publication_and_releases_gate() {
        let (u, probe) = probed_uploads();
        let (_, principals) = accounts(&u);
        let reader = principals.bound_reader(COOKIE);
        let authority = principals.clone();
        *probe.after_put.lock().expect("put hook") = Some(Box::new(move || {
            assert!(
                authority.command_gate(COOKIE).try_write().is_err(),
                "local publication must hold the authority read gate through staged storage"
            );
            // Preserve the exact epoch and principal but expire it while the
            // storage operation is finishing. No wall-clock sleeps are used.
            authority.expire_after(crate::identity::SessionLifetimes {
                idle: Duration::from_nanos(1),
                absolute: Duration::from_nanos(1),
            });
        }));
        principals.with_reader(COOKIE, &reader, || {
            let refused = post(&u, COOKIE, &png(9, 7), "expiring.png", "image/png");
            assert_ne!(
                refused.status, 303,
                "authority expiry during storage must refuse publication"
            );
        });
        assert!(
            principals.command_gate(COOKIE).try_write().is_ok(),
            "publication refusal releases its authority gate"
        );
        let state = shared(&u).state.lock().expect("state");
        assert!(state.leases.is_empty(), "expiry cannot leave a new lease");
        assert_eq!(state.staged_bytes, 0);
        assert!(state.holds.is_empty());
        drop(state);
        assert_eq!(
            probe.bytes.count().expect("count"),
            0,
            "expiry removes only its unheld staged bytes"
        );
        principals.expire_after(crate::identity::SessionLifetimes::default());
        let _ = attached(&u, &principals);
    }

    #[test]
    fn source_failure_after_staged_put_refuses_new_lease_and_removes_unheld_bytes() {
        let (u, probe) = probed_uploads();
        let (_, principals) = accounts(&u);
        let source = remote_source(&principals);
        let reader = principals.bound_reader(COOKIE);
        let authority = source.clone();
        *probe.after_put.lock().expect("put hook") = Some(Box::new(move || {
            authority
                .unavailable
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        principals.with_reader(COOKIE, &reader, || {
            assert_ne!(
                post(&u, COOKIE, &png(9, 7), "source.png", "image/png").status,
                303,
                "source failure during storage must refuse publication"
            );
        });
        let state = shared(&u).state.lock().expect("state");
        assert!(
            state.leases.is_empty(),
            "post-storage source failure publishes no lease"
        );
        assert_eq!(state.staged_bytes, 0);
        assert!(state.holds.is_empty());
        drop(state);
        assert_eq!(
            probe.bytes.count().expect("count"),
            0,
            "post-storage refusal removes newly unheld staged bytes"
        );
        source
            .unavailable
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = attached(&u, &principals);
    }

    #[test]
    fn source_failure_after_same_key_put_preserves_existing_lease_and_shared_bytes() {
        let (u, probe) = probed_uploads();
        let (_, principals) = accounts(&u);
        let source = remote_source(&principals);
        let (reader, shown) = attached(&u, &principals);
        let authority = source.clone();
        *probe.after_put.lock().expect("put hook") = Some(Box::new(move || {
            authority
                .unavailable
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        principals.with_reader(COOKIE, &reader, || {
            assert_ne!(
                post(&u, COOKIE, &png(9, 7), "same-key.png", "image/png").status,
                303,
                "failed same-key replacement must not publish"
            );
        });
        source
            .unavailable
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let preview = get(&u, &shown.src, COOKIE, "");
        assert_eq!(
            preview.status, 200,
            "failed replacement must preserve bytes already held by the original lease"
        );
        assert_eq!(preview.body, png(9, 7), "shared staged bytes remain exact");
        let state = shared(&u).state.lock().expect("state");
        assert_eq!(
            state.leases.len(),
            1,
            "failed replacement leaves just the original lease"
        );
        assert_eq!(state.leases[COOKIE].id, "up-1");
        assert_eq!(state.staged_bytes, png(9, 7).len() as u64);
        assert_eq!(state.holds.get(&BlobKey::of(&png(9, 7))), Some(&1));
        drop(state);
        assert_eq!(probe.bytes.count().expect("count"), 1);
    }

    #[test]
    fn preview_live_inspection_follows_the_complete_staged_blob_read() {
        let (u, probe) = probed_uploads();
        let (_, principals) = accounts(&u);
        let source = remote_source(&principals);
        let (reader, shown) = attached(&u, &principals);
        let authority = source.clone();
        let saved = source.identity.lock().expect("source").clone();
        *probe.after_get.lock().expect("get hook") = Some(Box::new(move || {
            *authority.identity.lock().expect("remote authority") = None;
        }));
        principals.with_reader(COOKIE, &reader, || {
            assert_eq!(get(&u, &shown.src, COOKIE, "").status, 404, "preview must revalidate authority after staged storage finishes reading private bytes");
        });
        *source.identity.lock().expect("source") = saved;
        let neighbor = get(&u, &shown.src, COOKIE, "");
        assert_eq!(
            neighbor.status, 200,
            "restored same generation can read its image"
        );
        assert_eq!(neighbor.body, png(9, 7));
    }

    #[test]
    fn a_lease_claimed_during_final_inspection_is_not_replaced() {
        let (u, probe) = probed_uploads();
        let (_, principals) = accounts(&u);
        let source = remote_source(&principals);
        let (reader, shown) = attached(&u, &principals);
        let leases = u.leases().required("leases");
        let claim = Arc::new(Mutex::new(None));
        let (authority, acquired) = (source.clone(), claim.clone());
        *probe.after_put.lock().expect("put hook") = Some(Box::new(move || {
            *authority.after_inspect.lock().expect("source hook") = Some(Box::new(move || {
                assert!(
                    leases.state.try_lock().is_ok(),
                    "final source inspection must not hold upload state while another command needs it"
                );
                *acquired.lock().expect("claim") = leases.claim(COOKIE);
            }));
        }));
        principals.with_reader(COOKIE, &reader, || {
            assert_ne!(
                post(&u, COOKIE, &png(5, 8), "incoming.png", "image/png").status,
                303,
                "an incoming upload cannot replace a lease claimed during source inspection"
            );
        });
        let claimed = claim
            .lock()
            .expect("claim")
            .take()
            .required("concurrent command claimed old lease");
        let leases = shared(&u);
        {
            let state = leases.state.lock().expect("state");
            assert_eq!(
                state.leases[COOKIE].id, "up-1",
                "claimed original lease is unchanged"
            );
            assert!(state.leases[COOKIE].claimed);
            assert_eq!(
                state.staged_bytes,
                png(9, 7).len() as u64,
                "refused reservation releases only its byte charge"
            );
            assert_eq!(state.holds.len(), 1);
            assert_eq!(state.holds.get(&BlobKey::of(&png(9, 7))), Some(&1));
        }
        leases.unclaim(claimed);
        assert_eq!(
            get(&u, &shown.src, COOKIE, "").status,
            200,
            "command rollback returns the original lease"
        );
        assert_eq!(probe.bytes.count().expect("count"), 1);
    }

    #[test]
    fn a_lease_replaced_during_final_inspection_is_not_overwritten() {
        let (u, probe) = probed_uploads();
        let (_, principals) = accounts(&u);
        let source = remote_source(&principals);
        let (reader, _) = attached(&u, &principals);
        let leases = u.leases().required("leases");
        let authority = source.clone();
        *probe.after_put.lock().expect("put hook") = Some(Box::new(move || {
            *authority.after_inspect.lock().expect("source hook") = Some(Box::new(move || {
                assert!(
                    leases.state.try_lock().is_ok(),
                    "replacement during source inspection must not wait on the upload state mutex"
                );
                let (ct, bytes) = form(&png(4, 6), "concurrent.png", "image/png");
                let headers = headers_for(&ct, bytes.len());
                let answer = leases.answer(
                    "POST",
                    "/uploads/post-image",
                    &headers,
                    COOKIE,
                    false,
                    &mut Cursor::new(bytes),
                );
                assert_eq!(answer.status, 303, "concurrent upload fixture succeeds");
            }));
        }));
        principals.with_reader(COOKIE, &reader, || {
            assert_ne!(post(&u, COOKIE, &png(5, 8), "superseded.png", "image/png").status, 303, "an incoming upload cannot overwrite a different lease installed during source inspection");
        });
        let current = shared(&u)
            .shown(COOKIE)
            .required("concurrent lease remains");
        assert_eq!(current.src, "/uploads/post-image/up-2");
        let preview = get(&u, &current.src, COOKIE, "");
        assert_eq!(preview.status, 200);
        assert_eq!(
            preview.body,
            png(4, 6),
            "superseded cleanup cannot delete the newer upload's bytes"
        );
        let state = shared(&u).state.lock().expect("state");
        assert_eq!(state.leases.len(), 1);
        assert_eq!(state.staged_bytes, png(4, 6).len() as u64);
        assert_eq!(state.holds.len(), 1);
        assert_eq!(state.holds.get(&BlobKey::of(&png(4, 6))), Some(&1));
        drop(state);
        assert_eq!(probe.bytes.count().expect("count"), 1);
    }

    struct HeldBody {
        bytes: Cursor<Vec<u8>>,
        reached: Option<mpsc::Sender<()>>,
        release: mpsc::Receiver<()>,
    }

    impl Read for HeldBody {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if let Some(reached) = self.reached.take() {
                reached
                    .send(())
                    .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
                self.release
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|_| std::io::ErrorKind::TimedOut)?;
            }
            self.bytes.read(out)
        }
    }

    impl BufRead for HeldBody {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            self.bytes.fill_buf()
        }
        fn consume(&mut self, n: usize) {
            self.bytes.consume(n);
        }
    }

    #[test]
    fn upload_held_in_body_cannot_publish_a_lease_after_completed_logout() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (identity, principals) = accounts(&u);
        // The same form succeeds with current authority before the race.
        let _ = attached(&u, &principals);
        let claimed = shared(&u)
            .claim(COOKIE)
            .required("clear the successful control");
        shared(&u).discarded(claimed);
        assert!(shared(&u).state.lock().expect("state").leases.is_empty());
        let reader = principals.bound_reader(COOKIE);
        let (ct, body) = form(&png(11, 6), "held.png", "image/png");
        let headers = headers_for(&ct, body.len());
        let (reached_tx, reached_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let u = &u;
            let principals = &principals;
            let worker = scope.spawn(move || {
                let mut body = HeldBody {
                    bytes: Cursor::new(body),
                    reached: Some(reached_tx),
                    release: release_rx,
                };
                principals.with_reader(COOKIE, &reader, || {
                    shared(u).answer(
                        "POST",
                        "/uploads/post-image",
                        &headers,
                        COOKIE,
                        false,
                        &mut body,
                    )
                })
            });
            reached_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("upload reached its body read");
            logout(&identity);
            release_tx.send(()).expect("release owned body reader");
            let answer = worker.join().expect("upload worker returned");
            assert_ne!(
                answer.status, 303,
                "an upload cannot publish after its reader was revoked"
            );
        });
        let state = shared(&u).state.lock().expect("state");
        assert!(
            state.leases.is_empty(),
            "revoked upload must publish no lease"
        );
        assert_eq!(
            state.staged_bytes, 0,
            "revoked upload must retain no staged bytes"
        );
        drop(state);
        assert_eq!(files(&d.path().join("staged")), 0);
        assert_eq!(files(&d.path().join("blobs")), 0);
    }
}
