use std::collections::HashMap;

use super::runtime::{ChatAttachment, ChatStreamEvent, ChatTurnSettings, CollabAgentStateInfo};

const ROOT_NODE_ID: &str = "swarm-root";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwarmNodeKind {
    Brain,
    Turn,
    Agent,
    Activity,
    Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwarmNodeStatus {
    Idle,
    Queued,
    Running,
    Waiting,
    Complete,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwarmNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: SwarmNodeKind,
    pub status: SwarmNodeStatus,
    pub title: String,
    pub subtitle: String,
    pub detail: String,
    pub model: Option<String>,
    pub reasoning: Option<String>,
    pub service_tier: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub long_context: bool,
    pub order: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwarmCanvasNode {
    pub node: SwarmNode,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwarmEdge {
    pub from_id: String,
    pub to_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwarmSnapshot {
    pub root_id: String,
    pub nodes: Vec<SwarmCanvasNode>,
    pub edges: Vec<SwarmEdge>,
    pub active_node_id: Option<String>,
    pub total_nodes: usize,
    pub active_nodes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwarmAgentTreeNode {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub detail: String,
    pub status: SwarmNodeStatus,
    pub model: Option<String>,
    pub reasoning: Option<String>,
    pub children: Vec<SwarmAgentTreeNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwarmProjection {
    root: SwarmNode,
    nodes: HashMap<String, SwarmNode>,
    turn_order: Vec<String>,
    child_order: HashMap<String, Vec<String>>,
    item_nodes: HashMap<String, String>,
    thread_nodes: HashMap<String, String>,
    active_turn_node_id: Option<String>,
    next_turn_index: usize,
}

impl SwarmProjection {
    pub fn new() -> Self {
        Self {
            root: SwarmNode {
                id: ROOT_NODE_ID.to_string(),
                parent_id: None,
                kind: SwarmNodeKind::Brain,
                status: SwarmNodeStatus::Idle,
                title: "Main Brain".to_string(),
                subtitle: "Standing by".to_string(),
                detail: "Send a message to begin the first process.".to_string(),
                model: None,
                reasoning: None,
                service_tier: None,
                thread_id: None,
                turn_id: None,
                long_context: false,
                order: 0,
            },
            nodes: HashMap::new(),
            turn_order: Vec::new(),
            child_order: HashMap::new(),
            item_nodes: HashMap::new(),
            thread_nodes: HashMap::new(),
            active_turn_node_id: None,
            next_turn_index: 1,
        }
    }

    pub fn start_turn(
        &mut self,
        prompt: &str,
        attachments: &[ChatAttachment],
        settings: &ChatTurnSettings,
    ) -> String {
        let turn_id = format!("turn-local-{}", self.next_turn_index);
        let attachment_count = attachments.len();
        let title = format!("Turn {:02}", self.next_turn_index);
        let prompt_excerpt = summarize_prompt(prompt, attachment_count);
        let subtitle = format_turn_settings(settings);
        let node = SwarmNode {
            id: turn_id.clone(),
            parent_id: Some(ROOT_NODE_ID.to_string()),
            kind: SwarmNodeKind::Turn,
            status: SwarmNodeStatus::Queued,
            title,
            subtitle,
            detail: prompt_excerpt,
            model: Some(settings.model.clone()),
            reasoning: settings.effort.clone(),
            service_tier: settings.service_tier.clone(),
            thread_id: self.root.thread_id.clone(),
            turn_id: None,
            long_context: settings.long_context,
            order: self.next_turn_index,
        };
        self.next_turn_index += 1;
        self.turn_order.push(turn_id.clone());
        self.nodes.insert(turn_id.clone(), node);
        self.child_order.entry(turn_id.clone()).or_default();
        self.active_turn_node_id = Some(turn_id.clone());
        self.root.status = SwarmNodeStatus::Queued;
        self.root.subtitle = "Preparing turn".to_string();
        self.root.detail = summarize_prompt_short(prompt, attachment_count);
        self.root.model = Some(settings.model.clone());
        self.root.reasoning = settings.effort.clone();
        self.root.service_tier = settings.service_tier.clone();
        self.root.long_context = settings.long_context;
        turn_id
    }

    pub fn interrupt_active(&mut self) {
        if let Some(turn_id) = self.active_turn_node_id.clone() {
            if let Some(turn) = self.nodes.get_mut(&turn_id) {
                turn.status = SwarmNodeStatus::Interrupted;
                turn.subtitle = "Interrupted by user".to_string();
            }
            self.root.status = SwarmNodeStatus::Interrupted;
            self.root.subtitle = "Interrupted".to_string();
        }
    }

    pub fn apply_chat_event(&mut self, event: &ChatStreamEvent) {
        match event {
            ChatStreamEvent::CodexRuntimeDialog { message } => {
                if let Some(message) = message {
                    self.root.subtitle = message.clone();
                }
            }
            ChatStreamEvent::ThreadReady { thread_id } => {
                self.root.thread_id = Some(thread_id.clone());
                if let Some(turn_id) = self.active_turn_node_id.clone() {
                    if let Some(turn) = self.nodes.get_mut(&turn_id) {
                        turn.thread_id = Some(thread_id.clone());
                    }
                }
            }
            ChatStreamEvent::TurnStarted { turn_id } => {
                if let Some(active_id) = self.active_turn_node_id.clone() {
                    if let Some(turn) = self.nodes.get_mut(&active_id) {
                        turn.status = SwarmNodeStatus::Running;
                        turn.turn_id = Some(turn_id.clone());
                        turn.subtitle = turn
                            .subtitle
                            .replace("Queued", "Running")
                            .replace("Preparing", "Running");
                    }
                }
                self.root.status = SwarmNodeStatus::Running;
                self.root.subtitle = "Running".to_string();
            }
            ChatStreamEvent::Status { message } => {
                if let Some(turn) = self.active_turn_mut() {
                    turn.status = if message.contains("Thinking") {
                        SwarmNodeStatus::Waiting
                    } else {
                        SwarmNodeStatus::Running
                    };
                    turn.subtitle = message.clone();
                }
                self.root.status = SwarmNodeStatus::Running;
                self.root.subtitle = message.clone();
            }
            ChatStreamEvent::Activity {
                item_id,
                title,
                detail,
                complete,
                ..
            } => {
                let node_id = self.ensure_item_node(
                    item_id,
                    title,
                    detail,
                    SwarmNodeKind::Activity,
                    *complete,
                );
                if let Some(node) = self.nodes.get_mut(&node_id) {
                    node.title = title.clone();
                    node.detail = detail.clone();
                    node.status = if *complete {
                        SwarmNodeStatus::Complete
                    } else {
                        SwarmNodeStatus::Running
                    };
                }
            }
            ChatStreamEvent::AgentThread { thread_id, label } => {
                self.ensure_agent_node(thread_id, label, None, None, &[], None);
            }
            ChatStreamEvent::CollabTool {
                item_id,
                title,
                detail,
                tool,
                sender_thread_id,
                receiver_thread_ids,
                model,
                reasoning_effort,
                agent_states,
                complete,
                ..
            } => {
                let node_id = self.ensure_item_node(
                    item_id,
                    title,
                    detail,
                    SwarmNodeKind::Activity,
                    *complete,
                );
                if let Some(node) = self.nodes.get_mut(&node_id) {
                    node.title = title.clone();
                    node.detail = detail.clone();
                    node.status = if *complete {
                        SwarmNodeStatus::Complete
                    } else {
                        SwarmNodeStatus::Running
                    };
                    node.subtitle = tool.clone();
                }
                for receiver_thread_id in receiver_thread_ids {
                    let label = self.agent_label(receiver_thread_id);
                    self.ensure_agent_node(
                        receiver_thread_id,
                        &label,
                        model.as_deref(),
                        reasoning_effort.as_deref(),
                        agent_states,
                        Some(sender_thread_id),
                    );
                }
                self.apply_agent_states(agent_states);
            }
            ChatStreamEvent::AssistantDelta { delta, .. } => {
                if let Some(turn) = self.active_turn_mut() {
                    turn.status = SwarmNodeStatus::Running;
                    append_preview(&mut turn.detail, delta);
                }
                self.root.subtitle = "Responding".to_string();
                append_preview(&mut self.root.detail, delta);
            }
            ChatStreamEvent::ReasoningDelta { delta, .. } => {
                if let Some(turn) = self.active_turn_mut() {
                    turn.status = SwarmNodeStatus::Waiting;
                    append_preview(&mut turn.subtitle, delta);
                }
                self.root.subtitle = "Thinking".to_string();
            }
            ChatStreamEvent::CommandStarted {
                item_id, command, ..
            } => {
                let node_id = self.ensure_item_node(
                    item_id,
                    "Command",
                    command,
                    SwarmNodeKind::Command,
                    false,
                );
                if let Some(node) = self.nodes.get_mut(&node_id) {
                    node.title = "$ Command".to_string();
                    node.detail = command.clone();
                    node.status = SwarmNodeStatus::Running;
                }
            }
            ChatStreamEvent::CommandDelta { item_id, delta } => {
                if let Some(node_id) = self.item_nodes.get(item_id).cloned() {
                    if let Some(node) = self.nodes.get_mut(&node_id) {
                        append_preview(&mut node.subtitle, delta);
                    }
                }
            }
            ChatStreamEvent::ItemDone { item_id } => {
                if let Some(node_id) = self.item_nodes.get(item_id).cloned() {
                    if let Some(node) = self.nodes.get_mut(&node_id) {
                        node.status = SwarmNodeStatus::Complete;
                    }
                }
            }
            ChatStreamEvent::TokenUsage {
                context_tokens,
                context_window,
                ..
            } => {
                self.root.detail = format!(
                    "Context {} / {}",
                    format_token_count(*context_tokens),
                    format_token_count(*context_window)
                );
            }
            ChatStreamEvent::Error { message } => {
                if let Some(turn) = self.active_turn_mut() {
                    turn.status = SwarmNodeStatus::Failed;
                    turn.subtitle = "Failed".to_string();
                    turn.detail = message.clone();
                }
                self.root.status = SwarmNodeStatus::Failed;
                self.root.subtitle = "Failed".to_string();
                self.root.detail = message.clone();
            }
            ChatStreamEvent::Completed => {
                if let Some(turn_id) = self.active_turn_node_id.take() {
                    if let Some(turn) = self.nodes.get_mut(&turn_id) {
                        if turn.status != SwarmNodeStatus::Failed
                            && turn.status != SwarmNodeStatus::Interrupted
                        {
                            turn.status = SwarmNodeStatus::Complete;
                            turn.subtitle = "Complete".to_string();
                        }
                    }
                }
                self.root.status = SwarmNodeStatus::Idle;
                self.root.subtitle = "Standing by".to_string();
            }
        }
    }

    pub fn snapshot(&self) -> SwarmSnapshot {
        const STAGE_CENTER_X: f32 = 700.0;
        const ROOT_WIDTH: f32 = 220.0;
        const ROOT_HEIGHT: f32 = 112.0;
        const AGENT_WIDTH: f32 = 220.0;
        const AGENT_HEIGHT: f32 = 104.0;

        let mut nodes = Vec::new();
        nodes.push(SwarmCanvasNode {
            node: self.root.clone(),
            x: STAGE_CENTER_X - ROOT_WIDTH * 0.5,
            y: 72.0,
            width: ROOT_WIDTH,
            height: ROOT_HEIGHT,
        });

        let mut edges = Vec::new();
        let mut agents = self
            .nodes
            .values()
            .filter(|node| node.kind == SwarmNodeKind::Agent)
            .cloned()
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.title.cmp(&right.title))
        });

        let columns = 4_usize;
        let col_spacing = 248.0;
        let row_spacing = 156.0;
        let start_y = 260.0;

        for (index, agent) in agents.iter().enumerate() {
            let row = index / columns;
            let col = index % columns;
            let column_center_offset = col as f32 - (columns as f32 - 1.0) * 0.5;
            let x = STAGE_CENTER_X - AGENT_WIDTH * 0.5 + column_center_offset * col_spacing;
            let y = start_y + row as f32 * row_spacing;
            nodes.push(SwarmCanvasNode {
                node: agent.clone(),
                x,
                y,
                width: AGENT_WIDTH,
                height: AGENT_HEIGHT,
            });
            edges.push(SwarmEdge {
                from_id: ROOT_NODE_ID.to_string(),
                to_id: agent.id.clone(),
            });
        }

        let active_nodes = nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.node.status,
                    SwarmNodeStatus::Queued | SwarmNodeStatus::Running | SwarmNodeStatus::Waiting
                )
            })
            .count();

        SwarmSnapshot {
            root_id: ROOT_NODE_ID.to_string(),
            total_nodes: nodes.len(),
            active_nodes,
            edges,
            nodes,
            active_node_id: agents
                .iter()
                .find(|node| {
                    matches!(
                        node.status,
                        SwarmNodeStatus::Queued
                            | SwarmNodeStatus::Running
                            | SwarmNodeStatus::Waiting
                    )
                })
                .map(|node| node.id.clone()),
        }
    }

    pub fn active_agent_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|node| node.kind == SwarmNodeKind::Agent)
            .filter(|node| {
                matches!(
                    node.status,
                    SwarmNodeStatus::Queued | SwarmNodeStatus::Running | SwarmNodeStatus::Waiting
                )
            })
            .count()
    }

    pub fn active_agent_tree(&self) -> Vec<SwarmAgentTreeNode> {
        let mut roots = self
            .nodes
            .values()
            .filter(|node| node.kind == SwarmNodeKind::Agent)
            .filter(|node| is_active_agent_status(node.status))
            .filter(|node| {
                node.parent_id.as_deref().is_none_or(|parent| {
                    parent == ROOT_NODE_ID || !self.is_active_agent_node(parent)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        roots.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.title.cmp(&right.title))
        });
        roots
            .into_iter()
            .map(|node| self.build_agent_tree(&node.id))
            .collect()
    }

    fn active_turn_mut(&mut self) -> Option<&mut SwarmNode> {
        let active_id = self.active_turn_node_id.clone()?;
        self.nodes.get_mut(&active_id)
    }

    fn ensure_item_node(
        &mut self,
        item_id: &str,
        title: &str,
        detail: &str,
        kind: SwarmNodeKind,
        complete: bool,
    ) -> String {
        if let Some(existing) = self.item_nodes.get(item_id).cloned() {
            return existing;
        }

        let Some(parent_id) = self.active_turn_node_id.clone() else {
            return ROOT_NODE_ID.to_string();
        };
        let node_id = format!("node-{item_id}");
        let order = self
            .child_order
            .get(&parent_id)
            .map(|children| children.len())
            .unwrap_or(0)
            + 1;
        let node = SwarmNode {
            id: node_id.clone(),
            parent_id: Some(parent_id.clone()),
            kind,
            status: if complete {
                SwarmNodeStatus::Complete
            } else {
                SwarmNodeStatus::Running
            },
            title: title.to_string(),
            subtitle: String::new(),
            detail: detail.to_string(),
            model: None,
            reasoning: None,
            service_tier: None,
            thread_id: self.root.thread_id.clone(),
            turn_id: self
                .nodes
                .get(&parent_id)
                .and_then(|parent| parent.turn_id.clone()),
            long_context: false,
            order,
        };
        self.nodes.insert(node_id.clone(), node);
        self.child_order
            .entry(parent_id)
            .or_default()
            .push(node_id.clone());
        self.item_nodes.insert(item_id.to_string(), node_id.clone());
        node_id
    }

    fn ensure_agent_node(
        &mut self,
        thread_id: &str,
        label: &str,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
        agent_states: &[CollabAgentStateInfo],
        sender_thread_id: Option<&str>,
    ) -> String {
        let parent_id = self.resolve_agent_parent_id(sender_thread_id);
        if let Some(existing) = self.thread_nodes.get(thread_id).cloned() {
            let parent_changed = self
                .nodes
                .get(&existing)
                .and_then(|node| node.parent_id.clone())
                != Some(parent_id.clone());
            if parent_changed {
                self.reparent_agent_node(&existing, &parent_id);
            }
            if let Some(node) = self.nodes.get_mut(&existing) {
                node.title = label.to_string();
                if let Some(model) = model {
                    node.model = Some(model.to_string());
                }
                if let Some(reasoning_effort) = reasoning_effort {
                    node.reasoning = Some(reasoning_effort.to_string());
                }
                node.parent_id = Some(parent_id);
            }
            return existing;
        }
        let node_id = format!("agent-{thread_id}");
        let order = self
            .child_order
            .get(&parent_id)
            .map(|children| children.len())
            .unwrap_or(0)
            + 1;
        let mut node = SwarmNode {
            id: node_id.clone(),
            parent_id: Some(parent_id.clone()),
            kind: SwarmNodeKind::Agent,
            status: SwarmNodeStatus::Queued,
            title: label.to_string(),
            subtitle: "Pending init".to_string(),
            detail: String::new(),
            model: model.map(ToOwned::to_owned),
            reasoning: reasoning_effort.map(ToOwned::to_owned),
            service_tier: None,
            thread_id: Some(thread_id.to_string()),
            turn_id: self
                .nodes
                .get(&parent_id)
                .and_then(|parent| parent.turn_id.clone()),
            long_context: false,
            order,
        };
        if let Some(state) = agent_states
            .iter()
            .find(|state| state.thread_id == thread_id)
        {
            apply_agent_state_to_node(&mut node, state);
        }
        self.nodes.insert(node_id.clone(), node);
        self.child_order
            .entry(parent_id)
            .or_default()
            .push(node_id.clone());
        self.thread_nodes
            .insert(thread_id.to_string(), node_id.clone());
        node_id
    }

    fn build_agent_tree(&self, node_id: &str) -> SwarmAgentTreeNode {
        let node = self
            .nodes
            .get(node_id)
            .cloned()
            .unwrap_or_else(|| SwarmNode {
                id: node_id.to_string(),
                parent_id: Some(ROOT_NODE_ID.to_string()),
                kind: SwarmNodeKind::Agent,
                status: SwarmNodeStatus::Queued,
                title: node_id.to_string(),
                subtitle: String::new(),
                detail: String::new(),
                model: None,
                reasoning: None,
                service_tier: None,
                thread_id: None,
                turn_id: None,
                long_context: false,
                order: 0,
            });
        let mut children = self
            .child_order
            .get(node_id)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|child_id| self.nodes.get(child_id))
            .filter(|child| {
                child.kind == SwarmNodeKind::Agent && is_active_agent_status(child.status)
            })
            .cloned()
            .collect::<Vec<_>>();
        children.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.title.cmp(&right.title))
        });
        SwarmAgentTreeNode {
            id: node.id,
            title: node.title,
            subtitle: node.subtitle,
            detail: node.detail,
            status: node.status,
            model: node.model,
            reasoning: node.reasoning,
            children: children
                .into_iter()
                .map(|child| self.build_agent_tree(&child.id))
                .collect(),
        }
    }

    fn is_active_agent_node(&self, node_id: &str) -> bool {
        self.nodes.get(node_id).is_some_and(|node| {
            node.kind == SwarmNodeKind::Agent && is_active_agent_status(node.status)
        })
    }

    fn resolve_agent_parent_id(&self, sender_thread_id: Option<&str>) -> String {
        if let Some(sender_thread_id) = sender_thread_id {
            if let Some(node_id) = self.thread_nodes.get(sender_thread_id) {
                return node_id.clone();
            }
        }
        ROOT_NODE_ID.to_string()
    }

    fn reparent_agent_node(&mut self, node_id: &str, new_parent_id: &str) {
        if let Some(current_parent_id) = self
            .nodes
            .get(node_id)
            .and_then(|node| node.parent_id.clone())
        {
            if let Some(children) = self.child_order.get_mut(&current_parent_id) {
                children.retain(|child_id| child_id != node_id);
            }
        }
        let new_order = self
            .child_order
            .get(new_parent_id)
            .map(|children| children.len())
            .unwrap_or(0)
            + 1;
        self.child_order
            .entry(new_parent_id.to_string())
            .or_default()
            .push(node_id.to_string());
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.parent_id = Some(new_parent_id.to_string());
            node.order = new_order;
        }
    }

    fn apply_agent_states(&mut self, agent_states: &[CollabAgentStateInfo]) {
        for state in agent_states {
            if let Some(node_id) = self.thread_nodes.get(&state.thread_id).cloned() {
                if let Some(node) = self.nodes.get_mut(&node_id) {
                    apply_agent_state_to_node(node, state);
                }
            }
        }
    }

    fn agent_label(&self, thread_id: &str) -> String {
        self.thread_nodes
            .get(thread_id)
            .and_then(|node_id| self.nodes.get(node_id))
            .map(|node| node.title.clone())
            .unwrap_or_else(|| {
                let suffix = thread_id.rsplit('-').next().unwrap_or(thread_id);
                format!("Agent {}", &suffix.chars().take(4).collect::<String>())
            })
    }
}

fn apply_agent_state_to_node(node: &mut SwarmNode, state: &CollabAgentStateInfo) {
    node.status = match state.status.as_str() {
        "pendingInit" => SwarmNodeStatus::Queued,
        "running" => SwarmNodeStatus::Running,
        "interrupted" => SwarmNodeStatus::Interrupted,
        "completed" => SwarmNodeStatus::Complete,
        "errored" | "notFound" => SwarmNodeStatus::Failed,
        "shutdown" => SwarmNodeStatus::Complete,
        _ => SwarmNodeStatus::Running,
    };
    node.subtitle = match state.status.as_str() {
        "pendingInit" => "Pending init",
        "running" => "Running",
        "interrupted" => "Interrupted",
        "completed" => "Completed",
        "errored" => "Errored",
        "shutdown" => "Shutdown",
        "notFound" => "Missing",
        _ => "Running",
    }
    .to_string();
    if let Some(message) = &state.message {
        node.detail = message.clone();
    }
}

fn is_active_agent_status(status: SwarmNodeStatus) -> bool {
    matches!(
        status,
        SwarmNodeStatus::Queued | SwarmNodeStatus::Running | SwarmNodeStatus::Waiting
    )
}

fn summarize_prompt(prompt: &str, attachment_count: usize) -> String {
    let trimmed = prompt.trim();
    let attachment_label = match attachment_count {
        0 => String::new(),
        1 => "1 attachment".to_string(),
        count => format!("{count} attachments"),
    };

    if trimmed.is_empty() {
        if attachment_label.is_empty() {
            "Prompt pending".to_string()
        } else {
            attachment_label
        }
    } else if attachment_label.is_empty() {
        trimmed.to_string()
    } else {
        format!("{trimmed} · {attachment_label}")
    }
}

fn summarize_prompt_short(prompt: &str, attachment_count: usize) -> String {
    excerpt(&summarize_prompt(prompt, attachment_count), 78)
}

fn format_turn_settings(settings: &ChatTurnSettings) -> String {
    let mut parts = vec![display_model(&settings.model)];
    if let Some(effort) = &settings.effort {
        parts.push(display_effort(effort));
    }
    if settings.service_tier.as_deref() == Some("fast") {
        parts.push("Fast".to_string());
    }
    if settings.long_context {
        parts.push("1M".to_string());
    }
    parts.join(" · ")
}

fn display_model(model: &str) -> String {
    match model {
        "gpt-5.4-pro" => "GPT-5.4 Pro".to_string(),
        "gpt-5.4" => "GPT-5.4".to_string(),
        "gpt-5.4-mini" => "GPT-5.4 Mini".to_string(),
        "gpt-5.4-nano" => "GPT-5.4 Nano".to_string(),
        other => other.to_string(),
    }
}

fn display_effort(effort: &str) -> String {
    match effort {
        "none" => "None",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "XHigh",
        other => other,
    }
    .to_string()
}

fn append_preview(target: &mut String, delta: &str) {
    if delta.trim().is_empty() {
        return;
    }

    if target.is_empty() {
        *target = excerpt(delta.trim(), 180);
        return;
    }

    let joined = format!("{} {}", target.trim(), delta.trim());
    *target = excerpt(&joined, 180);
}

fn excerpt(input: &str, max_chars: usize) -> String {
    let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text = compact.chars().take(max_chars).collect::<String>();
    if compact.chars().count() > max_chars {
        text.push('…');
    }
    text
}

fn format_token_count(value: u64) -> String {
    if value >= 1_000_000 {
        let amount = value as f64 / 1_000_000.0;
        let text = format!("{amount:.1}");
        format!("{}M", text.trim_end_matches('0').trim_end_matches('.'))
    } else if value >= 1_000 {
        let amount = value as f64 / 1_000.0;
        let text = format!("{amount:.1}");
        format!("{}K", text.trim_end_matches('0').trim_end_matches('.'))
    } else {
        value.to_string()
    }
}
