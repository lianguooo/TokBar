//! End-to-end pipeline test: scans whatever agent data exists on this
//! machine into a temp SQLite db and prints aggregate results.

use tokbar_lib::{aggregate, cost::CostMode, db, pricing::PricingMap};

#[test]
fn scan_and_aggregate_real_data() {
    let tmp = std::env::temp_dir().join("tokbar-test.db");
    let _ = std::fs::remove_file(&tmp);
    let mut conn = db::open(&tmp).expect("open db");
    let pricing = PricingMap::load(None);

    let stats = db::scan_all(&mut conn, &pricing, |_, _| {}).expect("scan");
    println!(
        "scan: {} files total, {} parsed, {} entries",
        stats.files_total, stats.files_parsed, stats.entries_inserted
    );

    let overview = aggregate::overview(&conn, None, None, CostMode::Auto).expect("overview");
    println!(
        "totals: cost=${:.4} tokens={} requests={} sessions={} days={}",
        overview.totals.cost,
        overview.totals.total_tokens,
        overview.totals.requests,
        overview.totals.sessions,
        overview.totals.active_days
    );
    for a in &overview.by_agent {
        println!(
            "  agent={} cost=${:.4} tokens={} requests={}",
            a.agent, a.cost, a.total_tokens, a.requests
        );
    }

    let models = aggregate::models(&conn, None, None, CostMode::Auto).expect("models");
    for m in models.iter().take(8) {
        println!(
            "  model={} cost=${:.4} in={} out={} cacheW={} cacheR={}",
            m.model,
            m.cost,
            m.input_tokens,
            m.output_tokens,
            m.cache_creation_tokens,
            m.cache_read_tokens
        );
    }

    let blocks = aggregate::blocks(&conn, None, CostMode::Auto, 5.0).expect("blocks");
    let usage_blocks = blocks.iter().filter(|b| !b.is_gap).count();
    println!("blocks: {} usage blocks", usage_blocks);

    // Cross-check: calculate mode should also produce a sane number.
    let calc = aggregate::overview(&conn, None, None, CostMode::Calculate).expect("calc");
    println!("calculate-mode cost=${:.4}", calc.totals.cost);

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn pricing_matches_claude_models() {
    let pricing = PricingMap::load(None);
    for model in [
        "claude-sonnet-4-5-20250929",
        "claude-opus-4-5-20251101",
        "claude-haiku-4-5-20251001",
        "gpt-5",
    ] {
        let p = pricing.find(model);
        assert!(p.is_some(), "no pricing match for {model}");
        let p = p.unwrap();
        assert!(p.input() > 0.0, "zero input rate for {model}");
        println!(
            "{model}: input={} output={} cacheW={} cacheR={}",
            p.input(),
            p.output(),
            p.cache_create(),
            p.cache_read()
        );
    }
}
