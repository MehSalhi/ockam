use core::sync::atomic::{AtomicU8, Ordering};
use core::time::Duration;
use ockam_core::compat::sync::Arc;
use ockam_core::{route, Address, AllowAll, Any, DenyAll, Mailboxes, Result, Routed, Worker};
use ockam_identity::access_control::IdentityAccessControlBuilder;
use ockam_identity::api::{DecryptionResponse, EncryptionRequest, EncryptionResponse};
use ockam_identity::{
    Identity, IdentitySecureChannelLocalInfo, TrustEveryonePolicy, TrustIdentifierPolicy,
};
use ockam_node::{Context, WorkerBuilder};
use ockam_vault::Vault;
use tokio::time::sleep;

#[ockam_macros::test]
async fn test_channel(ctx: &mut Context) -> Result<()> {
    let alice_vault = Vault::create();
    let bob_vault = Vault::create();

    let alice = Identity::create(ctx, alice_vault).await?;
    let bob = Identity::create(ctx, bob_vault).await?;

    let alice_trust_policy = TrustIdentifierPolicy::new(bob.identifier().clone());
    let bob_trust_policy = TrustIdentifierPolicy::new(alice.identifier().clone());

    bob.create_secure_channel_listener("bob_listener", bob_trust_policy)
        .await?;

    let alice_channel = alice
        .create_secure_channel(route!["bob_listener"], alice_trust_policy)
        .await?;

    let mut child_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "child",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    child_ctx
        .send(
            route![alice_channel, child_ctx.address()],
            "Hello, Bob!".to_string(),
        )
        .await?;

    let msg = child_ctx.receive::<String>().await?;

    let local_info = IdentitySecureChannelLocalInfo::find_info(msg.local_message())?;
    assert_eq!(local_info.their_identity_id(), alice.identifier());

    let return_route = msg.return_route();
    assert_eq!("Hello, Bob!", msg.body());

    child_ctx
        .send(return_route, "Hello, Alice!".to_string())
        .await?;

    let msg = child_ctx.receive::<String>().await?;

    let local_info = IdentitySecureChannelLocalInfo::find_info(msg.local_message())?;
    assert_eq!(local_info.their_identity_id(), bob.identifier());

    assert_eq!("Hello, Alice!", msg.body());

    ctx.stop().await
}

#[ockam_macros::test]
async fn test_channel_registry(ctx: &mut Context) -> Result<()> {
    let alice_vault = Vault::create();
    let bob_vault = Vault::create();

    let alice = Identity::create(ctx, alice_vault).await?;
    let bob = Identity::create(ctx, bob_vault).await?;

    bob.create_secure_channel_listener("bob_listener", TrustEveryonePolicy)
        .await?;

    let alice_channel = alice
        .create_secure_channel(route!["bob_listener"], TrustEveryonePolicy)
        .await?;

    let alice_channel_data = alice
        .secure_channel_registry()
        .get_channel_by_encryptor_address(&alice_channel)
        .unwrap();

    assert!(alice_channel_data.is_initiator());
    assert_eq!(alice_channel_data.my_id(), alice.identifier());
    assert_eq!(alice_channel_data.their_id(), bob.identifier());

    let mut bob_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "bob",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    ctx.send(
        route![alice_channel.clone(), "bob"],
        "Hello, Alice!".to_string(),
    )
    .await?;

    let msg = bob_ctx.receive::<String>().await?;
    let return_route = msg.return_route();

    assert_eq!("Hello, Alice!", msg.body());

    let bob_channel = return_route.next().unwrap().clone();

    let bob_channel_data = bob
        .secure_channel_registry()
        .get_channel_by_encryptor_address(&bob_channel)
        .unwrap();

    assert!(!bob_channel_data.is_initiator());
    assert_eq!(bob_channel_data.my_id(), bob.identifier());
    assert_eq!(bob_channel_data.their_id(), alice.identifier());

    ctx.stop().await
}

#[ockam_macros::test]
async fn test_channel_api(ctx: &mut Context) -> Result<()> {
    let alice_vault = Vault::create();
    let bob_vault = Vault::create();

    let alice = Identity::create(ctx, alice_vault).await?;
    let bob = Identity::create(ctx, bob_vault).await?;

    bob.create_secure_channel_listener("bob_listener", TrustEveryonePolicy)
        .await?;

    let alice_channel = alice
        .create_secure_channel(route!["bob_listener"], TrustEveryonePolicy)
        .await?;

    let mut bob_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "bob",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    ctx.send(
        route![alice_channel.clone(), "bob"],
        "Hello, Alice!".to_string(),
    )
    .await?;

    let msg = bob_ctx.receive::<String>().await?;
    let return_route = msg.return_route();

    assert_eq!("Hello, Alice!", msg.body());

    let bob_channel = return_route.next().unwrap().clone();

    let alice_channel_data = alice
        .secure_channel_registry()
        .get_channel_by_encryptor_address(&alice_channel)
        .unwrap();

    let bob_channel_data = bob
        .secure_channel_registry()
        .get_channel_by_encryptor_address(&bob_channel)
        .unwrap();

    let encrypted_alice: EncryptionResponse = ctx
        .send_and_receive(
            route![alice_channel_data.encryptor_api_address().clone()],
            EncryptionRequest(b"Ping".to_vec()),
        )
        .await?;
    let encrypted_alice = match encrypted_alice {
        EncryptionResponse::Ok(p) => p,
        EncryptionResponse::Err(err) => return Err(err),
    };

    let encrypted_bob: EncryptionResponse = ctx
        .send_and_receive(
            route![bob_channel_data.encryptor_api_address().clone()],
            EncryptionRequest(b"Pong".to_vec()),
        )
        .await?;
    let encrypted_bob = match encrypted_bob {
        EncryptionResponse::Ok(p) => p,
        EncryptionResponse::Err(err) => return Err(err),
    };

    let decrypted_alice: DecryptionResponse = ctx
        .send_and_receive(
            route![alice_channel_data.decryptor_api_address().clone()],
            encrypted_bob,
        )
        .await?;
    let decrypted_alice = match decrypted_alice {
        DecryptionResponse::Ok(p) => p,
        DecryptionResponse::Err(err) => return Err(err),
    };

    let decrypted_bob: DecryptionResponse = ctx
        .send_and_receive(
            route![bob_channel_data.decryptor_api_address().clone()],
            encrypted_alice,
        )
        .await?;
    let decrypted_bob = match decrypted_bob {
        DecryptionResponse::Ok(p) => p,
        DecryptionResponse::Err(err) => return Err(err),
    };

    assert_eq!(decrypted_alice, b"Pong");
    assert_eq!(decrypted_bob, b"Ping");

    ctx.stop().await
}

#[ockam_macros::test]
async fn test_tunneled_secure_channel_works(ctx: &mut Context) -> Result<()> {
    let vault = Vault::create();

    let alice = Identity::create(ctx, vault.clone()).await?;
    let bob = Identity::create(ctx, vault.clone()).await?;

    let alice_trust_policy = TrustIdentifierPolicy::new(bob.identifier().clone());
    let bob_trust_policy = TrustIdentifierPolicy::new(alice.identifier().clone());

    bob.create_secure_channel_listener("bob_listener", bob_trust_policy.clone())
        .await?;

    let alice_channel = alice
        .create_secure_channel(route!["bob_listener"], alice_trust_policy.clone())
        .await?;

    bob.create_secure_channel_listener("bob_another_listener", bob_trust_policy)
        .await?;

    let alice_another_channel = alice
        .create_secure_channel(
            route![alice_channel, "bob_another_listener"],
            alice_trust_policy,
        )
        .await?;

    let mut child_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "child",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    child_ctx
        .send(
            route![alice_another_channel, child_ctx.address()],
            "Hello, Bob!".to_string(),
        )
        .await?;
    let msg = child_ctx.receive::<String>().await?;
    let return_route = msg.return_route();
    assert_eq!("Hello, Bob!", msg.body());

    child_ctx
        .send(return_route, "Hello, Alice!".to_string())
        .await?;
    assert_eq!("Hello, Alice!", child_ctx.receive::<String>().await?.body());

    ctx.stop().await
}

#[ockam_macros::test]
async fn test_double_tunneled_secure_channel_works(ctx: &mut Context) -> Result<()> {
    let vault = Vault::create();

    let alice = Identity::create(ctx, vault.clone()).await?;
    let bob = Identity::create(ctx, vault.clone()).await?;

    let alice_trust_policy = TrustIdentifierPolicy::new(bob.identifier().clone());
    let bob_trust_policy = TrustIdentifierPolicy::new(alice.identifier().clone());

    bob.create_secure_channel_listener("bob_listener", bob_trust_policy.clone())
        .await?;

    let alice_channel = alice
        .create_secure_channel(route!["bob_listener"], alice_trust_policy.clone())
        .await?;

    bob.create_secure_channel_listener("bob_another_listener", bob_trust_policy.clone())
        .await?;

    let alice_another_channel = alice
        .create_secure_channel(
            route![alice_channel, "bob_another_listener"],
            alice_trust_policy.clone(),
        )
        .await?;

    bob.create_secure_channel_listener("bob_yet_another_listener", bob_trust_policy)
        .await?;

    let alice_yet_another_channel = alice
        .create_secure_channel(
            route![alice_another_channel, "bob_yet_another_listener"],
            alice_trust_policy,
        )
        .await?;

    let mut child_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "child",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    child_ctx
        .send(
            route![alice_yet_another_channel, child_ctx.address()],
            "Hello, Bob!".to_string(),
        )
        .await?;
    let msg = child_ctx.receive::<String>().await?;
    let return_route = msg.return_route();
    assert_eq!("Hello, Bob!", msg.body());

    child_ctx
        .send(return_route, "Hello, Alice!".to_string())
        .await?;
    assert_eq!("Hello, Alice!", child_ctx.receive::<String>().await?.body());

    ctx.stop().await
}

#[ockam_macros::test]
async fn test_many_times_tunneled_secure_channel_works(ctx: &mut Context) -> Result<()> {
    let vault = Vault::create();

    let alice = Identity::create(ctx, vault.clone()).await?;
    let bob = Identity::create(ctx, vault.clone()).await?;

    let alice_trust_policy = TrustIdentifierPolicy::new(bob.identifier().clone());
    let bob_trust_policy = TrustIdentifierPolicy::new(alice.identifier().clone());

    let n = rand::random::<u8>() % 5 + 4;
    let mut channels: Vec<Address> = vec![];
    for i in 0..n {
        bob.create_secure_channel_listener(i.to_string(), bob_trust_policy.clone())
            .await?;
        let channel_route = if i > 0 {
            route![channels.pop().unwrap(), i.to_string()]
        } else {
            route![i.to_string()]
        };
        let alice_channel = alice
            .create_secure_channel(channel_route, alice_trust_policy.clone())
            .await?;
        channels.push(alice_channel);
    }

    let mut child_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "child",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    child_ctx
        .send(
            route![channels.pop().unwrap(), child_ctx.address()],
            "Hello, Bob!".to_string(),
        )
        .await?;
    let msg = child_ctx.receive::<String>().await?;
    let return_route = msg.return_route();
    assert_eq!("Hello, Bob!", msg.body());

    child_ctx
        .send(return_route, "Hello, Alice!".to_string())
        .await?;
    assert_eq!("Hello, Alice!", child_ctx.receive::<String>().await?.body());

    ctx.stop().await
}

struct Receiver {
    received_count: Arc<AtomicU8>,
}

#[ockam_core::async_trait]
impl Worker for Receiver {
    type Message = Any;
    type Context = Context;

    async fn handle_message(
        &mut self,
        _context: &mut Self::Context,
        _msg: Routed<Self::Message>,
    ) -> Result<()> {
        self.received_count.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }
}

#[allow(non_snake_case)]
#[ockam_macros::test]
async fn access_control__known_participant__should_pass_messages(ctx: &mut Context) -> Result<()> {
    let received_count = Arc::new(AtomicU8::new(0));
    let receiver = Receiver {
        received_count: received_count.clone(),
    };

    let vault = Vault::create();

    let alice = Identity::create(ctx, vault.clone()).await?;
    let bob = Identity::create(ctx, vault.clone()).await?;

    let access_control = IdentityAccessControlBuilder::new_with_id(alice.identifier().clone());
    WorkerBuilder::with_access_control(
        Arc::new(access_control),
        Arc::new(DenyAll),
        "receiver",
        receiver,
    )
    .start(ctx)
    .await?;

    bob.create_secure_channel_listener("listener", TrustEveryonePolicy)
        .await?;

    let alice_channel = alice
        .create_secure_channel("listener", TrustEveryonePolicy)
        .await?;

    let child_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "child",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    child_ctx
        .send(route![alice_channel, "receiver"], "Hello, Bob!".to_string())
        .await?;

    sleep(Duration::from_secs(1)).await;

    assert_eq!(received_count.load(Ordering::Relaxed), 1);

    ctx.stop().await
}

#[allow(non_snake_case)]
#[ockam_macros::test]
async fn access_control__unknown_participant__should_not_pass_messages(
    ctx: &mut Context,
) -> Result<()> {
    let received_count = Arc::new(AtomicU8::new(0));
    let receiver = Receiver {
        received_count: received_count.clone(),
    };

    let vault = Vault::create();

    let alice = Identity::create(ctx, vault.clone()).await?;
    let bob = Identity::create(ctx, vault.clone()).await?;

    let access_control = IdentityAccessControlBuilder::new_with_id(bob.identifier().clone());
    WorkerBuilder::with_access_control(
        Arc::new(access_control),
        Arc::new(DenyAll),
        "receiver",
        receiver,
    )
    .start(ctx)
    .await?;

    bob.create_secure_channel_listener("listener", TrustEveryonePolicy)
        .await?;

    let alice_channel = alice
        .create_secure_channel("listener", TrustEveryonePolicy)
        .await?;

    let child_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "child",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    child_ctx
        .send(route![alice_channel, "receiver"], "Hello, Bob!".to_string())
        .await?;

    sleep(Duration::from_secs(1)).await;

    assert_eq!(received_count.load(Ordering::Relaxed), 0);

    ctx.stop().await
}

#[allow(non_snake_case)]
#[ockam_macros::test]
async fn access_control__no_secure_channel__should_not_pass_messages(
    ctx: &mut Context,
) -> Result<()> {
    let received_count = Arc::new(AtomicU8::new(0));
    let receiver = Receiver {
        received_count: received_count.clone(),
    };

    let access_control = IdentityAccessControlBuilder::new_with_id(
        "P79b26ba2ea5ad9b54abe5bebbcce7c446beda8c948afc0de293250090e5270b6".try_into()?,
    );
    WorkerBuilder::with_access_control(
        Arc::new(access_control),
        Arc::new(DenyAll),
        "receiver",
        receiver,
    )
    .start(ctx)
    .await?;

    let child_ctx = ctx
        .new_detached_with_mailboxes(Mailboxes::main(
            "child",
            Arc::new(AllowAll),
            Arc::new(AllowAll),
        ))
        .await?;

    child_ctx
        .send(route!["receiver"], "Hello, Bob!".to_string())
        .await?;

    sleep(Duration::from_secs(1)).await;

    assert_eq!(received_count.load(Ordering::Relaxed), 0);

    ctx.stop().await
}
