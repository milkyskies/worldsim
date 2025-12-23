# Realistic Conversation System Implementation Plan

**Goal**: Create a natural conversation system that ends based on participant mental states, not artificial turn limits. Future-proof for LLM integration.

---

## Design Philosophy

**No artificial limits.** Conversations end naturally based on:
- **Participant mental states**: Social drive, emotions, goals
- **Conversation content**: Pending questions, topics exhausted
- **External factors**: Urgent needs, danger, emergencies
- **Social dynamics**: Rudeness, relationship quality, personality

**LLM-ready data layer**: Captures structured conversation state (turns, intents, content) that an LLM can read to generate realistic dialogue.

---

## Problems Being Solved

1. **Conversations never end** → State never transitions to `Ended`
2. **Boring content** → Just "General" topic with no knowledge sharing
3. **Topic often None** → Rational brain creates Talk without topic, breaks `on_complete`
4. **No turn-taking awareness** → Agents don't track conversation obligations

---

## Core Data Model Changes

### Turn (add field)
```rust
pub struct Turn {
    pub speaker: Entity,
    pub intent: Intent,
    pub topic: Topic,
    pub content: Vec<Triple>,
    pub emotion: Option<Emotion>,
    pub timestamp: u64,
    pub expects_response: bool,     // NEW: Did this turn ask something?
}
```

### Conversation (add field)
```rust
pub struct Conversation {
    pub id: u64,
    pub participants: Vec<Entity>,
    pub turns: Vec<Turn>,
    pub state: ConversationState,
    pub started_at: u64,
    pub last_activity: u64,         // NEW: For stale detection
}
```

### InConversation (new component)
```rust
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct InConversation {
    pub conversation_id: u64,
    pub partner: Entity,
    pub my_turn: bool,
    pub owes_response: bool,        // Partner asked me something
}
```

---

## Natural Conversation Ending Rules

| Condition | Result | Relationship Impact |
|-----------|--------|---------------------|
| Both say Farewell | Clean end | +0.05 trust |
| Walk away after being asked | Rude exit | -0.15 trust, -0.10 affection |
| Walk away during Greeting | Very rude | -0.25 trust, -0.15 affection |
| Walk away during Wrapping | Acceptable | neutral |
| Stale (no activity 5+ sec) | Abandoned | -0.05 trust |
| Emergency (flee, pain) | Understandable | neutral |

---

## Why Conversations Continue

1. **Owes response**: Partner asked a question → must answer (or be rude)
2. **Has something to share**: Agent has relevant knowledge for current topic
3. **Social drive unsatisfied**: `social_drive > 0.2` and enjoying the interaction
4. **Relationship building**: High affection/trust target, want to deepen bond
5. **Low competing urgencies**: Nothing more urgent pulling them away

---

## Conversation State Machine

```
              ┌─────────────┐
    Start ──► │  GREETING   │ (1-2 turns: "Hello!" "Hi there!")
              └──────┬──────┘
                     │ both greeted
                     ▼
              ┌─────────────┐
              │   ACTIVE    │◄──────────────┐
              └──────┬──────┘               │
                     │                      │
         ┌───────────┼───────────┐          │
         │           │           │          │
         ▼           ▼           ▼          │
    [Share]      [Ask]      [Respond]───────┘
    knowledge   question    to question
         │           │           │
         └───────────┼───────────┘
                     │ ready to leave
                     ▼
              ┌─────────────┐
              │  WRAPPING   │ ("Well, I should go..." "Bye!")
              └──────┬──────┘
                     │ both said farewell
                     ▼
              ┌─────────────┐
              │   ENDED     │ (cleanup InConversation)
              └─────────────┘
```

---

## Implementation Steps

### 1. Enhance Turn struct
**File**: `src/agent/mind/conversation.rs`
- Add `expects_response: bool` field
- Set true when `intent == Intent::Ask`

### 2. Enhance Conversation struct
**File**: `src/agent/mind/conversation.rs`
- Add `last_activity: u64` field for stale detection
- Update on every turn added

### 3. Add InConversation component
**File**: `src/agent/mind/conversation.rs`
```rust
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct InConversation {
    pub conversation_id: u64,
    pub partner: Entity,
    pub my_turn: bool,
    pub owes_response: bool,
}
```

### 4. Fix TalkAction::to_template
**File**: `src/agent/actions/action/talk.rs`
- Override `to_template()` to set default `topic: Some(Topic::General)`
- Fixes bug where rational brain creates Talk with `topic: None`

### 5. Update TalkAction::on_complete
**File**: `src/agent/actions/action/talk.rs`
- Update `conversation.last_activity = current_tick`
- Set `turn.expects_response` based on intent
- Transition conversation state:
  ```rust
  c.state = match (c.state, intent) {
      (_, Intent::Farewell) => ConversationState::Ended,
      (ConversationState::Greeting, _) if c.turns.len() >= 2 => ConversationState::Active,
      (ConversationState::Active, _) if should_wrap => ConversationState::Wrapping,
      (state, _) => state,
  };
  ```
- If we asked a question, set partner's `owes_response = true`

### 6. Emotional brain conversation logic
**File**: `src/agent/brains/emotional.rs`

Add at START of `emotional_brain_propose()`:
```rust
if let Some(in_conv) = in_conversation {
    if in_conv.my_turn {
        // Priority 1: Must respond if we owe a response
        if in_conv.owes_response {
            return Some(propose_answer_turn(in_conv.partner, ...));
        }

        // Priority 2: Check if we want to leave
        let social_satisfied = social_drive < 0.2;
        let dominated_by_urgency = max_other_urgency > 60.0;

        if (social_satisfied || dominated_by_urgency) && !in_conv.owes_response {
            // Polite exit or rude walk-away based on agreeableness
            if agreeableness > 0.3 {
                return Some(propose_farewell(in_conv.partner, ...));
            } else {
                return None; // Rude exit - let other brain take over
            }
        }

        // Priority 3: Continue conversation
        return Some(propose_conversation_turn(in_conv.partner, mind, ...));
    }
}
```

### 7. Update brain_system
**File**: `src/agent/brains/brain_system.rs`
- Query for `Option<&InConversation>`
- Pass to `emotional_brain_propose()`

### 8. Conversation management systems
**File**: `src/agent/mind/conversation.rs`

Three new systems:

```rust
// Sync InConversation components when Talk actions complete
pub fn sync_conversation_state(
    mut commands: Commands,
    mut conv_manager: ResMut<ConversationManager>,
    agents: Query<(Entity, &ActionState), With<Agent>>,
) {
    // When Talk completes, update InConversation for both participants
    // Flip my_turn, update owes_response based on intent
}

// Clean up stale conversations
pub fn cleanup_stale_conversations(
    mut commands: Commands,
    mut conv_manager: ResMut<ConversationManager>,
    in_conversation: Query<(Entity, &InConversation)>,
    tick: Res<TickCount>,
) {
    // Mark conversations Ended if last_activity > 300 ticks (5 sec)
    // Remove InConversation components
}

// Apply relationship penalties for rude exits
pub fn handle_conversation_exits(
    mut events: EventReader<ConversationAbandoned>,
    mut agents: Query<&mut MindGraph, With<Agent>>,
) {
    // If someone walked away rudely, reduce trust/affection
}
```

### 9. Add Intent::Answer variant
**File**: `src/agent/mind/conversation.rs`
- For specifically responding to questions
- Helps track conversation flow

### 10. Register everything
**File**: `src/agent/mod.rs`
- Register `InConversation` component with reflection
- Add systems in order:
  1. `sync_conversation_state` (after action execution)
  2. `cleanup_stale_conversations` (periodic)
  3. `handle_conversation_exits` (event-driven)

---

## Files to Modify

| File | Changes |
|------|---------|
| `src/agent/mind/conversation.rs` | Add fields to Turn/Conversation, InConversation component, 3 new systems |
| `src/agent/actions/action/talk.rs` | Override to_template, enhance on_complete |
| `src/agent/brains/emotional.rs` | Add conversation decision logic |
| `src/agent/brains/brain_system.rs` | Query InConversation, pass to emotional |
| `src/agent/events.rs` | Add ConversationAbandoned event (optional) |
| `src/agent/mod.rs` | Register component and systems |

---

## Example Conversation Flow

```
Tick 100: Person A (social_drive=0.8) sees Person B (social_drive=0.5)

Tick 101: A.emotional_brain → Talk(B, Topic::General)
  → Creates conversation #42
  → A gets InConversation { my_turn: false, owes_response: false }
  → B gets InConversation { my_turn: true, owes_response: false }
  → Turn: Intent::Greet, expects_response: false

Tick 102: B.emotional_brain sees my_turn=true
  → Returns Talk proposal (urgency 70)
  → Turn: Intent::Greet, expects_response: false
  → State: Greeting → Active (2 greetings exchanged)
  → Flip turns: A.my_turn=true

Tick 103: A sees conversation, has knowledge about berries
  → Turn: Intent::Ask, Topic::Location(Berry), expects_response: true
  → B.owes_response = true

Tick 104: B.emotional_brain sees owes_response=true
  → Must answer! Returns Talk with berry location
  → Turn: Intent::Share, content: [Berry at (10,20)], expects_response: false
  → B.owes_response = false
  → A.my_turn = true

Tick 105: A receives info, social_drive drops to 0.5
  → Turn: Intent::Thank, expects_response: false

Tick 106: A shares gossip about wolf sighting
  → Turn: Intent::Share, Topic::Person(Wolf), expects_response: false
  → B.my_turn = true

Tick 107: B.social_drive now 0.2 (satisfied)
  → Emotional brain: social_satisfied=true, !owes_response
  → agreeableness=0.6 > 0.3 → Polite exit
  → Turn: Intent::Farewell
  → State: Active → Wrapping

Tick 108: A.my_turn=true, sees Wrapping state
  → Turn: Intent::Farewell
  → State: Wrapping → Ended
  → Cleanup: Remove InConversation from both

Total: 8 turns, ended naturally when B's social drive satisfied
```

---

## Future LLM Integration Design

The data layer provides everything an LLM needs:

**Input to LLM**:
```json
{
  "conversation": {
    "id": 42,
    "state": "Active",
    "turns": [
      {
        "speaker": "Person A",
        "intent": "Greet",
        "topic": "General",
        "content": [],
        "emotion": "Joy(0.3)",
        "expects_response": false
      },
      ...
    ],
    "my_turn": true,
    "owes_response": true,
    "partner": "Person B"
  },
  "my_state": {
    "social_drive": 0.6,
    "emotions": ["Joy(0.3)", "Curiosity(0.2)"],
    "relationship": {
      "trust": 0.7,
      "affection": 0.5,
      "respect": 0.6
    }
  },
  "my_knowledge": {
    "about_partner": [
      {"subject": "Person B", "predicate": "IsA", "object": "Person"},
      {"subject": "Person B", "predicate": "Knows", "object": true}
    ],
    "relevant_to_topic": [
      {"subject": "Berry", "predicate": "LocatedAt", "object": "Tile(10,20)"}
    ]
  }
}
```

**Output from LLM**:
```json
{
  "intent": "Share",
  "topic": "Location(Berry)",
  "content": [
    {
      "subject": "Berry",
      "predicate": "LocatedAt",
      "object": "Tile(10,20)"
    }
  ],
  "dialogue": "I saw some berries over by the river!",
  "emotion": "Helpful(0.4)"
}
```

**System processes**:
- Creates Turn from LLM output
- Updates conversation state
- Transfers knowledge to listener's MindGraph
- Updates relationship (trust +0.02 for helpful info)
- Flips turn to partner

---

## Testing Checklist

- [ ] Conversations end after farewell exchange
- [ ] Rude exits damage relationships
- [ ] Stale conversations (5+ sec) marked Ended
- [ ] `owes_response` prevents walking away mid-question
- [ ] Social drive satisfied → polite exit
- [ ] High urgency (hunger>80) can interrupt conversation
- [ ] Low agreeableness → rude exits more often
- [ ] Topics actually contain knowledge (not just "General")
- [ ] Turn-taking works (my_turn flips correctly)
- [ ] InConversation removed when conversation ends

---

## Success Metrics

**Before**: 131-turn conversations with no content, never ending
**After**:
- Natural 3-10 turn conversations based on social drive
- Knowledge actually shared (berries, wolves, emotions)
- Conversations end cleanly
- Relationship dynamics from exits (rude vs polite)
