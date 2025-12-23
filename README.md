# WorldSim

## 日本語

**WorldSim**は、スクリプトに頼らずに生物・心理・社会システムから行動が自然発生するエージェントシミュレーションです。

### コンセプト

「行動はすべて創発的に生まれる」という哲学に基づいています。

- **遺伝子が性格を形成** - 性格は割り当てられるのではなく、遺伝子と経験から計算される
- **記憶は不完全** - 誰もが出来事を違うふうに覚えている。歴史は嘘のコンセンサス
- **信念はゆっくり変化** - 確証バイアスは現実に存在する
- **人格は変わり得る** - トラウマ、喜び、「スナップ」が人を変える
- **社会は創発する** - 文化・派閥・歴史は個人から生まれる
- **構造は幻想** - 王国や宗教はエージェントがそう信じているからこそ存在する

### エージェント更新フロー

```
┌─────────────────────────────────────────────────────────────────┐
│                         TICK SYSTEM                              │
│  シミュレーションクロックを進め、時差更新をトリガー               │
└─────────────────────────────────────────────────────────────────┘
                            │
                ┌───────────┴───────────┐
                ▼                       ▼
┌──────────────────────────┐  ┌──────────────────────────┐
│      BIOLOGY SYSTEM      │  │    PERCEPTION SYSTEM     │
│                          │  │                          │
│  Body → Pain             │  │  Vision Range Check      │
│  Injuries → Healing      │  │  → VisibleObjects        │
│  Starvation → Damage     │  │                          │
│                          │  │  Body State Monitoring   │
│  OUTPUT:                 │  │  → Self beliefs          │
│  AgentState.Pain         │  │                          │
│  AgentState.Health       │  │  OUTPUT:                 │
└──────────────────────────┘  │  Perception Triples      │
                              └──────────────────────────┘
                │                       │
                └───────────┬───────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                      AGENT STATE                                 │
│  17変数: Hunger, Energy, Pain, Fear, Joy, etc.                  │
└─────────────────────────────────────────────────────────────────┘
                            │
                ┌───────────┴───────────┐
                ▼                       ▼
┌──────────────────────────┐  ┌──────────────────────────┐
│    NERVOUS SYSTEM        │  │       MINDGRAPH          │
│                          │  │                          │
│  1. Sync Emotions        │  │  Triple Store:           │
│  2. Activity Effects     │  │  - Perception beliefs    │
│  3. Generate Urgencies   │  │  - Episodic memories     │
│  4. Formulate Goals      │  │  - Semantic knowledge    │
│                          │  │  - Emotional associations│
│  OUTPUT:                 │  └──────────────────────────┘
│  Sorted Urgencies        │             │
│  Current Goal            │             │
└──────────────────────────┘             │
                │                        │
                └────────────┬───────────┘
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                      THREE BRAINS SYSTEM                         │
│                                                                  │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐   │
│  │ SURVIVAL BRAIN │  │ EMOTIONAL BRAIN│  │ RATIONAL BRAIN │   │
│  │                │  │                │  │                │   │
│  │ Checks:        │  │ Checks:        │  │ Checks:        │   │
│  │ - Pain > 70    │  │ - MindGraph    │  │ - Current Goal │   │
│  │ - Hunger > 80  │  │   associations │  │ - Plan valid?  │   │
│  │ - Energy < 15  │  │ - Fear/Joy/    │  │ - Replan if    │   │
│  │ - Fear > 0.8   │  │   Anger links  │  │   needed       │   │
│  │                │  │                │  │                │   │
│  │ Proposes:      │  │ Proposes:      │  │ Proposes:      │   │
│  │ Emergency acts │  │ Feeling acts   │  │ Planned acts   │   │
│  └────────────────┘  └────────────────┘  └────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│                     ┌────────────────┐                          │
│                     │  ARBITRATION   │                          │
│                     │                │                          │
│                     │ Vote: Urgency  │                          │
│                     │     × Power    │                          │
│                     │                │                          │
│                     │ Winner Selected│                          │
│                     └────────────────┘                          │
│                                                                  │
│  OUTPUT: BrainState.chosen_action                               │
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ACTION EXECUTION                              │
│                                                                  │
│  chosen_action → CurrentActivity + TargetPosition に変換         │
└─────────────────────────────────────────────────────────────────┘
```

### 三つの脳

三層脳理論にインスパイアされた、並列的な意思決定システム。三つの脳がそれぞれ提案を出し、重み付き投票によって最終的な行動が選ばれる。

- **生存脳**: 痛み、飢餓、睡眠などの緊急反応（Power: 1.0, 常時）
- **感情脳**: MindGraphの感情連想に基づく行動（Power: ムード変動 + 痛み）
- **理性脳**: GOAP A* 計画による複数ステップの推論（Power: エネルギー + 覚醒度）

調停スコア = 緊急度 × Power

### 記憶とナレッジ

エージェントは**MindGraph**を維持 - すべての知識のためのセマンティック・トリプル・ストア:

| 記憶タイプ | 半減期 | 例 |
|---------|--------|-----|
| Intrinsic | ∞ (減衰なし) | 永続的特性 |
| Semantic | 10時間 | 「狼は危険」 |
| Episodic | 5分（基本） | 「昨日ボブを見た」 |
| Perception | 30秒 | 「リンゴの木は(5,3)にある」 |

### エージェント状態

各エージェントは **17変数** を追跡:

**身体的ニーズ**: Hunger, Thirst, Energy, Health, Pain  
**心理的欲求**: Social, Fun, Curiosity, Status, Security, Autonomy  
**精神状態**: Stress, Alertness  
**感情**: Fear, Anger, Joy, Sadness

---

## English

**WorldSim** is an agent simulation where behavior emerges naturally from biological, psychological, and social systems—not from scripts.

### Concept

Built on the philosophy that "all behavior emerges from systems."

- **Genes shape personality** - Traits are calculated from genetics + experiences, not assigned
- **Memory is unreliable** - Everyone remembers events differently; history is a consensus of lies
- **Beliefs update slowly** - Confirmation bias is real
- **Personality can change** - Trauma, joy, and "The Snap" reshape people
- **Society emerges** - Culture, factions, and history arise from individuals
- **Structures are hallucinations** - Kingdoms and religions exist only because agents believe they do

### Complete Agent Update Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                         TICK SYSTEM                              │
│  Advances simulation clock, triggers staggered updates           │
└─────────────────────────────────────────────────────────────────┘
                            │
                ┌───────────┴───────────┐
                ▼                       ▼
┌──────────────────────────┐  ┌──────────────────────────┐
│      BIOLOGY SYSTEM      │  │    PERCEPTION SYSTEM     │
│                          │  │                          │
│  Body → Pain             │  │  Vision Range Check      │
│  Injuries → Healing      │  │  → VisibleObjects        │
│  Starvation → Damage     │  │                          │
│                          │  │  Body State Monitoring   │
│  OUTPUT:                 │  │  → Self beliefs          │
│  AgentState.Pain         │  │                          │
│  AgentState.Health       │  │  OUTPUT:                 │
└──────────────────────────┘  │  Perception Triples      │
                              └──────────────────────────┘
                │                       │
                └───────────┬───────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                      AGENT STATE                                 │
│  17 variables: Hunger, Energy, Pain, Fear, Joy, etc.            │
└─────────────────────────────────────────────────────────────────┘
                            │
                ┌───────────┴───────────┐
                ▼                       ▼
┌──────────────────────────┐  ┌──────────────────────────┐
│    NERVOUS SYSTEM        │  │       MINDGRAPH          │
│                          │  │                          │
│  1. Sync Emotions        │  │  Triple Store:           │
│  2. Activity Effects     │  │  - Perception beliefs    │
│  3. Generate Urgencies   │  │  - Episodic memories     │
│  4. Formulate Goals      │  │  - Semantic knowledge    │
│                          │  │  - Emotional associations│
│  OUTPUT:                 │  └──────────────────────────┘
│  Sorted Urgencies        │             │
│  Current Goal            │             │
└──────────────────────────┘             │
                │                        │
                └────────────┬───────────┘
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                      THREE BRAINS SYSTEM                         │
│                                                                  │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐   │
│  │ SURVIVAL BRAIN │  │ EMOTIONAL BRAIN│  │ RATIONAL BRAIN │   │
│  │                │  │                │  │                │   │
│  │ Checks:        │  │ Checks:        │  │ Checks:        │   │
│  │ - Pain > 70    │  │ - MindGraph    │  │ - Current Goal │   │
│  │ - Hunger > 80  │  │   associations │  │ - Plan valid?  │   │
│  │ - Energy < 15  │  │ - Fear/Joy/    │  │ - Replan if    │   │
│  │ - Fear > 0.8   │  │   Anger links  │  │   needed       │   │
│  │                │  │                │  │                │   │
│  │ Proposes:      │  │ Proposes:      │  │ Proposes:      │   │
│  │ Emergency acts │  │ Feeling acts   │  │ Planned acts   │   │
│  └────────────────┘  └────────────────┘  └────────────────┘   │
│                              │                                  │
│                              ▼                                  │
│                     ┌────────────────┐                          │
│                     │  ARBITRATION   │                          │
│                     │                │                          │
│                     │ Vote: Urgency  │                          │
│                     │     × Power    │                          │
│                     │                │                          │
│                     │ Winner Selected│                          │
│                     └────────────────┘                          │
│                                                                  │
│  OUTPUT: BrainState.chosen_action                               │
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ACTION EXECUTION                              │
│                                                                  │
│  Translates chosen_action → CurrentActivity + TargetPosition    │
└─────────────────────────────────────────────────────────────────┘
```

### Three Brains Architecture

Inspired by triune brain theory - three parallel decision-making systems that each propose actions. A weighted voting system determines the final winner.

- **Survival Brain**: Emergency responses for pain, hunger, sleep (Power: 1.0, always active)
- **Emotional Brain**: Association-driven behavior from MindGraph (Power: |Mood Swing| + Pain)
- **Rational Brain**: GOAP A* planning with multi-step reasoning (Power: Energy + Alertness)

Arbitration Score = Urgency × Power

### Memory & Knowledge

Agents maintain a **MindGraph** - a semantic triple store for all knowledge:

| Memory Type | Half-Life | Example |
|------------|-----------|---------|
| Intrinsic | ∞ (never decays) | Permanent traits |
| Semantic | 10 hours | "Wolves are dangerous" |
| Episodic | 5 minutes base | "I saw Bob yesterday" |
| Perception | 30 seconds | "Apple tree is at (5,3)" |

### Agent State

Each agent tracks **17 variables**:

**Physical Needs**: Hunger, Thirst, Energy, Health, Pain  
**Psychological Drives**: Social, Fun, Curiosity, Status, Security, Autonomy  
**Mental State**: Stress, Alertness  
**Emotions**: Fear, Anger, Joy, Sadness
