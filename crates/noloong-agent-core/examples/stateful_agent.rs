use noloong_agent_core::{
    Agent, AgentEventKind, BoxFuture, CancellationToken, ModelProvider, ModelRequest,
    ModelStreamEvent, ModelStreamSink, StopReason,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[tokio::main]
async fn main() -> noloong_agent_core::Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(TurnCountingModel::default()))
        .build()?;

    agent.subscribe(|event| async move {
        if matches!(event.kind, AgentEventKind::RunCompleted) {
            println!("run completed");
        }
        Ok(())
    });
    agent.follow_up(noloong_agent_core::AgentMessage::user(
        "follow-up-example",
        "one more turn",
    ));

    agent.prompt("hello").await?;
    println!("messages: {}", agent.state().await.messages.len());
    Ok(())
}

#[derive(Default)]
struct TurnCountingModel {
    calls: AtomicU64,
}

impl ModelProvider for TurnCountingModel {
    fn id(&self) -> &str {
        "turn-counting"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let turn = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: format!("turn-{turn}"),
                },
                ModelStreamEvent::TextDelta {
                    text: format!("response {turn}"),
                },
                ModelStreamEvent::Finished {
                    stop_reason: StopReason::Stop,
                },
            ];
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}
