<!-- semantic-code-memory:begin -->
## 业务语义记忆

- 回答或修改业务能力、跨服务流程、数据表归属等问题前，先调用 `semantic_context`，再进行大范围源码检索；中文问题提供英文标识符、模块名或业务别名作为 `searchHints`。
- `MISS`、`PARTIAL`、`STALE`，或本轮编辑了已绑定证据后，完成探索必须调用 `semantic_memory_patch` 提交结算。
- `ACTIVE` 命中可直接复用；不得编造 Symbol 或 TABLE 证据。
<!-- semantic-code-memory:end -->
