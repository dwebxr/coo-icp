# coo-icp — 全体コードレビュー & リファクタリング計画

対象: `src/eliza_backend/src/lib.rs`（単一 8,621 行）+ フロントエンド + ビルド/Candid/docs。
mainnet デプロイ済み（`canister_ids.json: 4wfup-gqaaa-aaaas-qdqca-cai`）。

調査体制: Sonnet サブエージェント6体で範囲分割レビュー + Web調査1体（ICP 最新ベストプラクティス）。頭出し所見は Fable が実コードで直接照合済み。

---

## 0. 全体所見（フレーミング）

- 直近コミットは "v0.5.0: production hardening" だが、**中核の暗号処理に致命的な正しさ・機密性バグが複数**（後述）。ウォレット機能は**実資金で本番稼働した形跡が疑わしい**（EVM tx は RLP バグで原理上ブロードキャスト失敗、SPL は ATA 導出が偽物）。
- したがって計画の第一歩は「**どの機能が実際に mainnet で動いているかの経験的確認**」。RLP/ATA バグが本当なら該当機能は一度も成功していないはずで、その事実が投資優先度を決める。
- 8,621 行モノリスは wasm ハード上限（10MiB code / 100MiB total）には遠いが、`StableState` 全 candid シリアライズ pre/post_upgrade は **DFINITY が公式に非推奨とするレガシー・アンチパターン**。段階移行が最優先アーキ課題。

---

## 1. Critical（正しさ・セキュリティ）— リファクタ前 or 並行で必須

### C-1. EVM RLP エンコード欠陥（全 EVM tx が失敗）【実在確認済み】
- accessList を `rlp_encode_bytes(&[])`=`0x80`（空文字列）で符号化。EIP-1559 は空リスト=`0xc0` 必須。該当: lib.rs:3253（署名preimage）, 3823（send_evm_native）, 3986（send_erc20）, 4330（execute_lifi_bridge）, 4600（execute_uniswap_swap）, 6297（erc8004 contract call）。
- 署名 r/s を生32バイトのまま `rlp_encode_bytes` に渡し先頭ゼロ非除去（3821-3826 ほか各所）。RLP整数は最小符号化必須。~1/128 tx が拒否。
- **修正**: 署名preimage と署名済みtx組立の両方で `rlp_encode_list(&[])` を使用し、r/s は先頭ゼロ除去してから符号化。両者は必ず同一エンコードでないと署名が一致しない。→ 後述の共通ヘルパ `sign_and_broadcast_eip1559` に集約して1箇所で直す。

### C-2. Solana ATA 導出が偽物（全 SPL 機能が破綻）
- `derive_associated_token_account`（lib.rs:5373-5395）は単純 SHA256、bump 探索・off-curve チェック無し。実 ATA（`find_program_address`）と一致しない。
- 影響: `send_spl_token` は存在しないアカウント参照で失敗、`get_spl_token_balance` は実残高を "0" と誤報。
- **修正**: 正しい PDA 導出（bump をデクリメント探索し ed25519 曲線外まで反復）を実装。または SPL 機能自体が未使用なら明示的に削除。

### C-3. Solana 秘密鍵の XOR「暗号化」= 機密性ゼロ【実在確認済み】
- `get_encryption_key`（4737）は公開値 `ic_cdk::id()` のみから鍵導出。誰でも鍵を再計算し `encrypted_secret_key` から生秘密鍵を復元可能。
- **修正（推奨・最有力）**: ローカル鍵保存を全廃し **threshold Ed25519（`sign_with_schnorr`, algorithm=ed25519, key_1）** に移行。mainnet GA 済み・DFINITY公式パターン（sol-rpc-canister の basic_solana 例が根拠）。鍵流出リスク・XOR偽装・TOCTOU（C-5）を一挙に解消。コスト ~26B cycles/署名。

### C-4. API キー / SNS 資格情報が平文保存【実在確認済み】
- `decrypt_api_key`（1074）/`decrypt_bytes`（1385）は no-op UTF-8 変換。OpenAI キー・Twitter secret・Discord bot token が平文で state → StableMemory に保存。README:1540「API keys encrypted with vetKeys」は**虚偽**。
- **修正**: 実暗号化を導入するまで README/命名から "encrypted" を撤去し、単一の技術的負債項目として明示追跡。恒久解は vetKeys（vetKD）統合。ingress 引数は E2E 暗号化されない点も設計上の限界として明記。

### C-5. `init_solana_wallet` の TOCTOU 競合（資金喪失）
- lib.rs:4755-4791: `already_initialized` チェックが `raw_rand().await` の前、ロック無し。並行 update 呼び出しで2つの鍵を生成、後勝ちで一方のアドレスへ送った資金が回収不能。
- **修正**: await 前に in-flight ロック（RAII guard, Drop で解放）を立てる。C-3 の threshold 移行で鍵生成自体が不要になれば根本解消。

### C-6. EVM nonce ロック無し・await 越しの再入競合
- `get_nonce`（3335）は毎回 RPC から "pending" 取得。`send_evm_native`/`send_erc20`/`execute_lifi_bridge`/`execute_uniswap_swap`/`send_evm_contract_call` は複数 await を跨ぐが `EvmWalletState`（248-253）にロック field 無し。並行呼び出しで同一 nonce 取得 → 片方の tx が黙って落ちる。
- **修正**: per-wallet の nonce 予約 + in-flight ロック（RAII）。DFINITY セキュリティ指針「await ごとに『別メッセージが同じ state を読み書きできるか』を問う」に沿った監査を全 fund-moving メソッドに実施。

### C-7. アップグレードでタイマーが再開されない（機能が黙って死ぬ）
- post_upgrade（758-833）が polling/auto_post の config を復元するが `start_polling`/`start_auto_posting` を呼ばずタイマー再作成しない。config は `enabled=true` を報告し続けるが実際は停止。
- **修正**: post_upgrade 末尾で config を見て必要なタイマーを再武装。`health()` にタイマー稼働状況を含める。

### C-8. `chat()` の匿名プリンシパル未拒否 + レート制限欠如
- CONVERSATIONS を `caller` キーで管理するが `Principal::anonymous()` を弾かない → 全匿名ユーザが会話履歴を共有・漏洩（get_conversation_history）。
- `RateLimiter`（151）は存在するが `chat()` から未参照 → 無認証で LLM cycles / 50B cycles outcall を無制限に誘発（cycle-drain DoS）。
- **修正**: 匿名拒否 or セッション設計見直し、`chat()` にレート制限適用。

### C-9. ERC-8004 `configure_erc8004_registry` の default 二重化【実在確認済み】
- 更新分岐（6012 `state.registries[idx] = config`）が他 registry の `is_default` を解除しない。複数 default → 全 consumer の `.find(|r| r.is_default)` が非決定的に別 contract/chain を選ぶ（実 on-chain tx の宛先が壊れる）。
- **修正**: 更新・新規いずれの経路でも「default 設定時は他を解除」を一元化した setter に。

---

## 2. Important（正しさ・堅牢性・認可）

### 外部変更 outcall と consensus / 冪等性（要経験的確認）
- I-1. **POST outcall の consensus 破綻リスク**: replicated 実行では各 replica が実 HTTP を送信。Twitter OAuth nonce は time 由来で replica 間同一 → 最初の1件のみ受理・他は replay 拒否 → 応答が不一致で consensus 失敗の恐れ（post_tweet 1443, send_discord_message 1758 ほか）。**まず「自律投稿が現に mainnet で成功しているか」を確認**してから対処方針決定。
- I-2. OAuth nonce が body/POST params を含まない（1299）→ 同一秒の連投で nonce 衝突 → 2件目が replay 拒否。
- I-3. `transform_solana_response`（4859）が `result.context.slot` 等の可変フィールドを除去しない → getBalance/getLatestBlockhash 等で consensus 失敗の恐れ。transform は header 以外の可変 body も正規化必須。
- I-4. 全 outcall が冪等でない。tweet 投稿・署名済み tx ブロードキャストには**自前の冪等キー/dedup 層**が必要（プラットフォームは保証しない）。

### メッセージ処理の取りこぼし・二重処理
- I-5. `mark_message_replied`（2362）が schedule 失敗時も無条件実行 → 返信が永久ドロップ、リトライ経路無し。
- I-6. `process_scheduled_posts`/`process_incoming_messages` に再入ガード無し（snapshot→逐次mark）→ タイマー重複発火や手動 trigger で二重投稿。
- I-7. `generate_social_response` 失敗前に `processed=true`（2327）→ 一過性 LLM 障害でメッセージ永久ドロップ（ScheduledPost にはある retry 機構が inbox に無い）。
- I-8. rate-limit カウンタが試行時に消費（1410）→ 失敗連発で成功ゼロのまま枠枯渇。

### EVM/DEX 実行の正しさ
- I-9. `execute_uniswap_swap`/`execute_lifi_bridge` が swap 前に ERC-20 `approve` を発行しない → on-chain revert。だが `send_raw_transaction` は JSON-RPC error のみ検査し実行status未確認 → `Ok`/`Submitted` と誤報（false success）。
- I-10. tx status が EVM/Solana とも `Submitted` から進まない。`wait_for_transaction_receipt`（3522）等の receipt 確認ヘルパが存在するのに未使用。
- I-11. Uniswap router/quoter アドレスがグローバル定数（4382）で chain 別でない。Base 等は別アドレス → 誤/存在しない contract を叩く。chain_id→address テーブル必要。
- I-12. `send_icp`（2833）が `created_at_time: None` → ICP ledger の重複検出が無効 → リトライ/再入で二重送金の恐れ。

### 認可・情報開示
- I-13. `get_scheduled_posts`/`get_incoming_messages` が admin ゲート無し `#[query]`【実在確認済み】→ 未公開下書き・SNS メッセージ内容/作者が匿名者に漏洩。
- I-14. ERC-8004 の read 系（`erc8004_get_agent_info` ほか）が `#[query]` でなく無認証 `#[update]` → 誰でも 30-50B cycles の outcall を無制限誘発（cycle-drain griefing）。
- I-15. **単一 admin・全か無か**。`require_admin` は単一 `CONFIG.admin` 等値のみ。fund-moving 全経路がこれ1つに依存 → admin 鍵1つの侵害で全資金ドレイン/`reset_solana_wallet` による鍵消去（バックアップ無し）。multisig/timelock/role 分離を将来課題として明記。
- I-16. **whitelist バイパス**: `validate_against_whitelist` は `execute_command` からのみ呼ばれ、`send_icp`/`send_evm_*`/bridge/swap を直接呼べば素通り。各 fund-moving メソッド自身に検査を移すか、直接 export を廃し execute_command 経由に統一。

### ERC-8004 データ整合
- I-17. `erc8004_register_agent`（6162）が metadata を on-chain 送信時に長さ0固定で符号化 → on-chain へは metadata が書かれず local cache とだけ食い違う（silent data loss）。
- I-18. `erc8004_get_agent_info`（6357）が on-chain 読み取り結果を捨て cache のみで応答。自分以外の agent_id を照会できず、無駄に 50B cycles 消費。

### 状態・アップグレード安全性
- I-19. `StableState` の大半 field に `#[serde(default)]` 無し → 将来の非 Option field 追加で旧 state の `candid::decode_one` が破綻。`version` field は分岐に未使用の死重。
- I-20. post_upgrade の length-prefix が実 stable memory サイズと未照合（769）→ 破損時 `stable_read` が trap し、default fallback の意図に反して upgrade 全体が失敗。

---

## 3. Nit（軽微・スタイル・堅牢性）

- OAuth param ソートが percent-encode 前キーで実施（1324, RFC 5849 非準拠だが現状 ASCII のみで無害）。
- byte 長 vs char 長の不整合（2556/2068, 非 ASCII で過剰/過小トリム）。
- `ic_random` 時刻シード getrandom shim がグローバル登録（4708）→ 将来任意コードが弱乱数を黙って受け取る地雷。現状は raw_rand 経路で未使用。
- Solana 秘密鍵バッファ非ゼロ化（`drop(signing_key)` は no-op コメント, 5061）。
- Solana compact-u16 長を単一 byte 決め打ち（4977/5398）→ 値≥128 で破綻。
- `decompress_pubkey` が曲線上検証なし（2952）。
- `get_execution_history` の doc（最新順）と実装（古い順）不一致（7492）。
- `MAX_EXECUTION_HISTORY` トリムが `Vec::remove(0)` で O(n)（7476）→ `VecDeque` 推奨。
- `Erc8004State.feedback_cache`（384）死蔵。
- JSON-RPC body を一部 `format!` 文字列結合（4083/4436）で構築（他は `serde_json::json!`）。
- 一部 EVM 応答を parse 済み Value でなく生 body の文字列検索で抽出（get_erc20_balance/get_uniswap_quote）。
- `logout()` に try/catch 無し（App.tsx:153）→ UI が「認証済み」で固着し得る。
- message `key` 冗長（App.tsx:213, 無害）。

---

## 4. リファクタリング（構造）— モジュール分割 & 重複排除

### 4.1 重複排除（バグ修正の受け皿にもなる）
- **`require_admin` の inline 再実装 ~15+ 箇所** → 既存 `require_admin()`（1370）へ統一。将来の admin モデル変更を1箇所に。
- **JSON-RPC リクエスト boilerplate ~30 箇所（EVM+Solana）** → `async fn evm_rpc_call(...)` / `async fn solana_json_rpc(...)` に集約。transform 正規化もここで一元化（I-3 の受け皿）。
- **EIP-1559 sign+broadcast パイプライン 4-5 重複** → `async fn sign_and_broadcast_eip1559(...)`。C-1 の RLP 修正をここ1箇所で。
- **ERC-8004 3 registry が 80% 同一構造** → `erc8004::abi`（`selector`/`encode_uint256`/`encode_address`/`encode_string`、既存 `keccak256_selector`(3047) を全 inline 複製から呼ぶ）+ `AbiWordReader` で復号を集約。
- **Solana tx-history append 3x / submit-tx 3x / `decode_solana_pubkey` の inline 複製** → ヘルパ集約。
- **social HTTP outcall boilerplate 6x** → `social_http_request(...)` + `SOCIAL_OUTCALL_CYCLES` 定数 + `social_transform_context()`。
- **動的 serde_json indexing 多数** → typed `#[derive(Deserialize)]` 応答 struct へ。
- **cycles 定数 `50_000_000_000` の散在** → 名前付き定数。

### 4.2 モジュール分割（8,621 行 → 複数モジュール / 段階的に複数 crate も検討）
提案構成（各ドメインが自分の型 + thread_local + `capture()/restore()` を所有し、pre/post_upgrade はそれを呼ぶだけにする）:
- `agent_core`（chat/character/config/conversation, LLM 推論）
- `social`（oauth / twitter / discord / scheduler / autopost / inbox / admin_api）
- `wallet_icp`, `wallet_evm`（+ `evm_rpc`, `evm_signing`）, `wallet_solana`（+ `solana_rpc`）
- `erc8004`（identity / reputation / validation / abi）
- `governance`（whitelist / execute_command）
- `diagnostics`（health / version / metrics — 全ドメインの counter を登録制で集約）
- `crypto`/`util`（keccak, rlp, hex, base58, address 導出）
- `state`（MemoryManager と各ドメインの stable 構造を束ねる）

### 4.3 アーキ移行（Web 調査に基づく高価値項目）
- **ic-stable-structures v0.7.x + MemoryManager** へ段階移行。ドメインごとに `StableBTreeMap`/`StableCell`（各 unique `MemoryId`, 領域共有は厳禁）。モノリシック candid pre/post_upgrade を廃し upgrade を O(小) 化・成長を非有界化。C-7/I-19/I-20 もこの過程で解消。
- **threshold Ed25519（`sign_with_schnorr`）** で Solana ローカル鍵を全廃（C-3/C-5 根本解消）。
- **公式 EVM RPC canister（7hfb6-…）/ SOL RPC canister（tghme-…）** で手書き JSON-RPC outcall を置換 → single-provider 依存排除・consensus/retry 内蔵・bespoke transform 大幅削減（I-3 の一部も解消）。`ic-alloy` は EVM tx 組立/署名の置換候補（C-1/C-6 を成熟ライブラリで代替）。
- **ic-cdk 0.18.3 への依存更新**（0.18 で `Call` builder API に全面変更）。call サイト移行は独立した機械的ステップとして計画。0.18.0-0.18.2 は避け 0.18.3 pin。

---

## 5. 整合性・ハイジーン

- **D2A Briefing API が幽霊【実在確認済み】**: candid（.did 8メソッド+型）/README:1554-1612/Makefile:214-258 に有るが lib.rs に実装ゼロ。→ 実装するか candid/README/Makefile から全撤去（現状 call 時エラー）。
- **Candid 3 重ドリフト**: 手書き `.did`(620) / 生成 `.did`(605, `get_metrics` 欠落=stale) / フロント手書き TS subset(101, 12メソッドのみ)。→ `dfx generate` 出力をフロントが参照するよう配線（vite/tsconfig alias + `make build` で同期）、手書き subset を廃止。
- **死蔵依存**: `rlp`(Cargo.toml:32), `rand`(38) 直接未使用 → `cargo udeps` 確認の上削除（wasm サイズ敏感）。
- **リポジトリ肥大**: `node_modules/` が commit 済み(~148MB, .gitignore は後付け) → `git rm -r --cached`。`overview.png`(9.4MB) は LFS/縮小検討。`.gitignore` の `Cargo.lock` 行は矛盾（canister は commit が正）→ 行削除。
- **テスト実行経路欠如**: 121 個の `#[test]` があるが Makefile/CI に `cargo test` target 無し（`make test` は health call のみ）→ target 追加で腐敗防止。
- **README 1,628 行**は API doc 重複でドリフト温床（既に vetKeys/D2A で虚偽記載）→ candid 生成物へのリンクに置換し重複削減。

---

## 6. 実行フェーズ & 分業（このMac 共通ワークフロー準拠）

分業: Sonnet=内部調査 / X Search=外部 → **Fable=設計（本書）** → **Codex(`/gpt-plan-review`)=計画ゲート**（Critical/Important 全対応まで反復）→ **Codex(GPT-5.5 high)=実装**（特に難所は Fable 直接）→ **Fable=監査（build/test/実行時スモーク）** → **Codex(`/gpt-review`)=コードゲート** → リリース。Codex 上限時は **Opus 4.8 サブエージェント**で代替。

### Phase 0 — 経験的確認（実装前・破壊なし・最優先）【合否基準付き】
- **EVM/SPL 成功実績の判定手段（READ-ONLY で具体化）**: `get_evm_wallet_info`/`EVM_WALLET_STATE.transaction_history`(250)/`tx_counter`(251)/`get_solana_transaction_history` を candid query で採取 → 導出アドレス（`get_evm_address`/`get_solana_address` 4798）を block explorer で照合 → testnet で 1 tx replay。**合否基準**: history に成功記録が無い、あっても explorer 上に該当 tx が存在しなければ「C-1/C-2 により未成功」確定。
- **アドレス上の残高確認（PC-3 の前提）**: EVM/Solana 導出アドレスの on-chain 残高を explorer で確認。Solana に残高があれば threshold 移行前に sweep 手順が必須（後述 §8-PC-3）。
- **テストベースライン採取**: `PATH="$HOME/.cargo/bin:$PATH" cargo test` を現状で走らせ、121 個の `#[test]` が現に pass するか採取。腐敗（compile 不能/失敗）していればリファクタのガードに使えないので、まず緑化。
- どの機能が現に使われているか（social 自律投稿 / wallet / erc8004 / governance）の棚卸し。使われていない surface は「修正」でなく「削除」候補（C-1/C-2 両方に適用＝削除 gate を揃える）。

### Phase 1 — Critical 修正（安全第一、小さく検証可能な単位で）
1. C-4/C-3 secrets: README/命名の虚偽表記を即撤去（誤認防止）。Solana threshold Ed25519 移行は**アドレス変更を伴うため §8-PC-3 の sweep 手順とセットで Phase 3**。Phase 1 は虚偽表記撤去のみ。
2. C-1 RLP: `rlp_encode_list(&[])` + r/s 先頭ゼロ除去。**検証は署名済み typed-tx の全バイトを viem/ethers 参照値と等値比較**（calldata 等値では本バグを捕まえられない＝PI-2）。可能なら testnet ノードが実際に受理するまで確認。**難所につき Fable 直接実装も可**。C-1 も Phase 0 で EVM 未使用と判明すれば「削除」分岐対象（C-2 と gate を揃える＝PI-6）。この tx ベクタテストは Phase 3 の ic-alloy 置換の受け入れ検証にそのまま再利用（＝Phase 1 の手書き修正が捨て仕事になっても、テスト資産は残る＝PI-1）。
3. C-9 default 二重化 / C-8 匿名拒否+レート制限（**公式フロントは II 認証必須なので匿名拒否は既存フロー非破壊＝実物確認済み PI-5**）/ C-5・C-6 ロック（RAII guard）。
4. **C-7 タイマー再武装は I-1/I-2（POST outcall consensus/replay）が未解決の間は自動発火させない**。post_upgrade の再武装をフラグで gate し、I-1 を Phase 0 で判定・Phase 2 で解決してから有効化（＝PI-4 の順序制約）。`get_social_status` に `AUTO_POST_TIMER_ID` も露出（現状 auto-post タイマー死はどの query でも観測不能）。
5. C-2 ATA: 正しい PDA 導出 or 機能削除（Phase 0 の使用状況で判断）。

### Phase 2 — Important（認可・取りこぼし・consensus/冪等）
- I-13/I-14 認可ゲート、I-16 whitelist 経路統一、I-5/I-6/I-7 メッセージ処理、I-9/I-10/I-11 EVM 実行正しさ、I-3/I-4 transform/冪等、I-12 created_at_time、I-17/I-18 ERC-8004 データ整合。

### Phase 3 — 構造リファクタ（機能不変を保証しつつ）
- **順序**: (a) ic-cdk 0.17→0.18.3 バンプ + `Call` builder API 移行を**先頭**に（sign_with_schnorr / sol-rpc / ic-alloy が 0.18 系 API 前提なら依存的に必須）。→ (b) 4.1 重複排除 → (c) 4.2 モジュール分割 → (d) §8 の移行（stable-structures / threshold Ed25519 / RPC canister 置換）。
- 4.1 重複排除（sign_and_broadcast / rpc_call / abi ヘルパ）→ Critical 修正の再発防止装置。
- 4.2 モジュール分割（1コミット1ドメイン、candid/挙動不変を各段で監査）。ソース上はドメイン単位で進めてよいが、**stable-structures への runtime カットオーバーは全ドメイン一括**（§8-PC-1、混在は物理的に不能）。
- 4.3 の危険手順は独立章 §8 に詳細化（後方互換・trap semantics・アドレス変更）。

### Phase 4 — 整合性・ハイジーン
- D2A 幽霊 API 撤去、Candid 生成パイプライン一本化、死蔵依存削除、node_modules/overview.png、cargo test の CI target、README スリム化。

### 各 Phase の監査（Fable）
- `PATH="$HOME/.cargo/bin:$PATH"` で cargo build/test（Homebrew Rust と衝突注意）、`dfx build`、candid 再生成の差分確認、可能なら local replica 実行時スモーク。
- 各 Phase 完了時に `/gpt-review` でブランチ差分ゲート（Critical/Bug/Security 全対応まで反復）。

---

## 7. 未確認事項（Phase 0 で潰す）
- EVM/SPL/自律投稿の mainnet 実成功実績（C-1/C-2/I-1 の実害範囲を確定）。
- ~~`ic-cdk` 現行バージョン~~ → **確認済み**: 宣言 `0.17`、ロックファイル解決 `0.16.1`（重複の可能性）。ic-cdk-timers 0.10.1 / candid 0.10.20。0.18 系 `Call` API 移行は実在する作業。
- 各機能の使用状況（削除 vs 修正の判断）。
- 公式 EVM RPC(7hfb6-…)/SOL RPC(tghme-…) canister id は依存前に実 id を再確認（本 offline 環境で未検証）。

---

## 8. 移行安全性（計画ゲート Critical 反映 — mainnet 実 state を壊さないための核心手順）

現状の永続化方式を実物確認済み: pre_upgrade が `candid::encode` した**単一 blob を stable memory offset 0（長さ prefix at 0, データ at 8）に生書き込み**、post_upgrade がそれを読む（lib.rs:748-773）。`ic-stable-structures`/`MemoryManager` は未使用（grep 0件）。

### PC-1. stable-structures 移行は「一回限り・全ドメイン一括カットオーバー」
- **問題**: `MemoryManager::init` は page 0 に自前ヘッダを置くため、既存の生 candid blob（offset 0）と物理衝突する。かつ「一部ドメインが MM 仮想メモリ・他が offset 0」の共存は不能。→ 当初案「ドメイン単位の runtime 段階移行」は誤り。
- **移行 upgrade の post_upgrade 手順（順序厳守）**:
  1. **先に**旧 length-prefix 方式で生 stable memory から旧 blob を読み、`candid::decode_one::<StableState>` で thread_local へ復元（MM を触る前に）。
  2. **その後**に `MemoryManager` を初期化（page 0 ヘッダを上書き）し、各ドメインの `StableBTreeMap`/`StableCell`（各 unique `MemoryId`）へ書き移す。
  3. 以降の upgrade では旧経路を撤去。
- **リハーサル必須**: local replica に本番相当の旧 blob dump を流し込み → 移行 upgrade → 全ドメイン state の等値確認。ぶっつけ本番 upgrade 禁止。

### PC-2. アップグレード失敗の trap semantics / ロールバック
- post_upgrade が trap すると **upgrade 全体が失敗し旧コードのまま残る（暗黙ロールバック）**。C-7 修正・PC-1 移行はともに post_upgrade を書き換えるため中心論点。
- **防御（移行コードの一部として必須）**:
  - `len` を `stable_size() * 65536` と照合してから `stable_read`（現状 769 は未照合＝I-20）。
  - decode 失敗時は panic せず既存 default fallback を維持。
  - 各 Phase の post_upgrade 変更につき「失敗しても旧 state が保たれる」ことを local replica（旧 blob 投入→upgrade→等値）で検証。

### PC-3. threshold Ed25519 移行は Solana アドレスが変わる（EVM は不変にできる）
- **Solana（不可避のアドレス変更）**: `sign_with_schnorr(ed25519)` の鍵はローカル生成鍵と別鍵 → 公開鍵＝アドレスが必ず変わる。旧 `cached_address`(4790) 上の資金と外部参照が壊れる。
  - 手順: Phase 0 で旧アドレス残高を確認 →(a) 残高ありなら threshold 新アドレスへ **sweep 完了を確認してから**旧鍵経路を撤去、(b) 残高ゼロなら破棄。アドレス移行の外部周知も。
- **EVM（アドレス不変にできる＝invariant として固定）**: 実物確認済み — `get_evm_address`(3057)/`sign_with_chain_key_ecdsa`(3265) は同一 `key_id`(get_ecdsa_key_id 2943) + `derivation_path = vec![canister_id.as_slice().to_vec()]`(3068/3265) で導出。ic-alloy 置換時に**同一 key_id・同一 derivation_path を維持すればアドレス不変**。移行後にアドレス等値をアサートするテストを付ける。
- この非対称性（EVM=不変可 / Solana=不可避変更）を計画の明示的な不変条件として固定。

---

## 9. 計画ゲート結果（`/gpt-plan-review` → Codex 上限のため Opus 4.8 で代替、実物照合込み）
- **技術的主張 9/9 が実コードで CONFIRMED**（C-9 のみ「更新分岐限定」で PARTIAL だが特定は正確）。誤検知・誇張なし。C-1 は「一部でなく全 EVM 送金経路が原理上ブロードキャスト不能」と規模確定。
- ゲートの Critical 3件（PC-1/PC-2/PC-3）= 上記 §8 に反映。Important 6件（PI-1〜PI-6）= Phase 0/1/3 の各記述に反映済み。
- 骨子（Phase 0 経験確認 → Critical 修正 → 重複排除 → 構造/stable 移行）は妥当と評価。**Critical/Important 全対応済み → 実装フェーズ（Codex or Fable）へ進める状態**（Codex 上限リセットは 7/9 22:50）。
