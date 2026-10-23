use super::*;

#[tokio::test]
async fn real_queued_completion_is_discarded_on_cancel_or_authority_loss() {
    for cancel in [true, false] {
        let f = Fixture::new(1, false);
        let url = f.url();
        let (mut s, t, b) = setup(&url);
        start(&mut s, &t, 1, &url, &[]).await;
        let wire::HttpReply::Head { head } = reply(&mut s, &t).await else {
            panic!("head");
        };
        grant(&mut s, &t, head.id, 0, false).unwrap();
        assert!(matches!(
            reply(&mut s, &t).await,
            wire::HttpReply::Chunk { offset: 0, .. }
        ));
        grant(&mut s, &t, head.id, 1, false).unwrap();
        wait_workers(&b).await;
        // Observe a real completed production worker with its unconsumed terminal event.
        let owner = &s.documents[s.index(&t).unwrap()];
        let State::Running(r) = &owner.requests[&id(1)].state else {
            panic!("running");
        };
        assert!(r.operation.is_finished());
        assert_eq!(r.operation.events.len(), 1);
        assert_eq!(f.count(), 1);
        assert!(b.retained_payload_bytes() > 0);
        if cancel {
            handle(
                &mut s,
                &t,
                wire::HttpRequest::Cancel {
                    id: id(1),
                    source_generation: 7,
                },
            )
            .unwrap();
            assert!(
                matches!(reply(&mut s,&t).await,wire::HttpReply::Cancelled{response:Some(r),..} if r==head.id)
            );
            assert!(
                s.poll(&t, |_| true, MAX_FRAME_LEN as usize + 5)
                    .unwrap()
                    .is_none()
            );
            assert!(handle(&mut s, &t, wire::HttpRequest::Reuse { response: head.id }).is_err());
            assert_eq!(b.retained_payload_bytes(), 0);
            s.retire(&t);
        } else {
            assert!(matches!(
                s.poll(&t, |_| false, MAX_FRAME_LEN as usize + 5),
                Err(core::Error::Stale)
            ));
            assert!(handle(&mut s, &t, wire::HttpRequest::Reuse { response: head.id }).is_err());
        }
        wait_workers(&b).await;
        assert_eq!(b.retained_payload_bytes(), 0);
    }
}
