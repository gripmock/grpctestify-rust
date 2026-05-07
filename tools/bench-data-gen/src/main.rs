use anyhow::Result;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

const TIERS: &[(&str, usize)] = &[
    ("1kb", 1 * 1024),
    ("128kb", 128 * 1024),
    ("512kb", 512 * 1024),
    ("1mb", 1024 * 1024),
    ("4mb", 4 * 1024 * 1024),
    ("100mb", 100 * 1024 * 1024),
    ("1gb", 1024 * 1024 * 1024),
];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let out_dir = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".tmp/bench-data"));
    let only_tier = args.get(2).map(String::as_str);

    for (tier_name, target_bytes) in TIERS {
        if let Some(filter) = only_tier
            && filter != *tier_name
        {
            continue;
        }
        generate_tier(&out_dir, tier_name, *target_bytes)?;
    }

    println!("Generated bench datasets in {}", out_dir.display());
    println!("Run example:");
    println!("  cargo run --manifest-path tools/bench-data-gen/Cargo.toml --release -- .tmp/bench-data");
    println!("  cargo run --manifest-path tools/bench-data-gen/Cargo.toml -- .tmp/bench-data 1mb");
    Ok(())
}

fn generate_tier(root: &Path, tier_name: &str, target_bytes: usize) -> Result<()> {
    let tier_dir = root.join(tier_name);
    let data_dir = tier_dir.join("data");
    let bench_dir = tier_dir.join("bench");
    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(&bench_dir)?;

    let zones = (target_bytes / 4096).clamp(8, 100_000);
    let customers = (target_bytes / 64).clamp(32, 2_000_000);
    let shipments = (target_bytes / 32).clamp(64, 3_000_000);
    let items = (target_bytes / 2048).clamp(16, 200_000);
    let depots = (target_bytes / 48).clamp(64, 3_000_000);

    write_csv_set(&data_dir, zones, customers, shipments, items, depots)?;
    write_tsv_set(&data_dir, zones, customers, shipments, items, depots)?;
    write_ndjson_set(&data_dir, zones, customers, shipments, items, depots)?;

    write_bench_files(&bench_dir, "csv")?;
    write_bench_files(&bench_dir, "tsv")?;
    write_bench_files(&bench_dir, "ndjson")?;

    println!(
        "tier={tier_name} target={}KB customers={} shipments={} depots={}",
        target_bytes / 1024,
        customers,
        shipments,
        depots
    );
    Ok(())
}

fn write_csv_set(
    dir: &Path,
    zones: usize,
    customers: usize,
    shipments: usize,
    items: usize,
    depots: usize,
) -> Result<()> {
    let mut w = BufWriter::new(File::create(dir.join("service_zones.csv"))?);
    writeln!(w, "zone_id,zone_name,tier")?;
    for i in 1..=zones {
        writeln!(w, "z{i},Zone {i},{}", tier(i))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("customers.csv"))?);
    writeln!(w, "customer_id,zone_id,status")?;
    for i in 1..=customers {
        writeln!(w, "c{i},z{},{}", 1 + (i % zones), customer_status(i))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("shipments.csv"))?);
    writeln!(w, "shipment_id,customer_id,zone_id,total")?;
    for i in 1..=shipments {
        let cid = 1 + (i % customers);
        let zid = 1 + (cid % zones);
        writeln!(w, "s{i},c{cid},z{zid},{}", 100 + (i % 9000))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("catalog_items.csv"))?);
    writeln!(w, "item_id,category")?;
    for i in 1..=items {
        writeln!(w, "i{i},cat{}", 1 + (i % 50))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("customer_items.csv"))?);
    writeln!(w, "customer_id,item_id")?;
    for i in 1..=shipments {
        let cid = 1 + (i % customers);
        let iid = 1 + ((i * 17) % items);
        writeln!(w, "c{cid},i{iid}")?;
    }

    let mut w = BufWriter::new(File::create(dir.join("depots.csv"))?);
    writeln!(w, "depot_id,zone_id,city,load_bucket")?;
    for i in 1..=depots {
        writeln!(w, "d{i},z{},{} ,{}", 1 + (i % zones), city(i), i % 100)?;
    }
    Ok(())
}

fn write_tsv_set(
    dir: &Path,
    zones: usize,
    customers: usize,
    shipments: usize,
    items: usize,
    depots: usize,
) -> Result<()> {
    let mut w = BufWriter::new(File::create(dir.join("service_zones.tsv"))?);
    writeln!(w, "zone_id\tzone_name\ttier")?;
    for i in 1..=zones {
        writeln!(w, "z{i}\tZone {i}\t{}", tier(i))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("customers.tsv"))?);
    writeln!(w, "customer_id\tzone_id\tstatus")?;
    for i in 1..=customers {
        writeln!(w, "c{i}\tz{}\t{}", 1 + (i % zones), customer_status(i))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("shipments.tsv"))?);
    writeln!(w, "shipment_id\tcustomer_id\tzone_id\ttotal")?;
    for i in 1..=shipments {
        let cid = 1 + (i % customers);
        let zid = 1 + (cid % zones);
        writeln!(w, "s{i}\tc{cid}\tz{zid}\t{}", 100 + (i % 9000))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("catalog_items.tsv"))?);
    writeln!(w, "item_id\tcategory")?;
    for i in 1..=items {
        writeln!(w, "i{i}\tcat{}", 1 + (i % 50))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("customer_items.tsv"))?);
    writeln!(w, "customer_id\titem_id")?;
    for i in 1..=shipments {
        let cid = 1 + (i % customers);
        let iid = 1 + ((i * 17) % items);
        writeln!(w, "c{cid}\ti{iid}")?;
    }

    let mut w = BufWriter::new(File::create(dir.join("depots.tsv"))?);
    writeln!(w, "depot_id\tzone_id\tcity\tload_bucket")?;
    for i in 1..=depots {
        writeln!(w, "d{i}\tz{}\t{}\t{}", 1 + (i % zones), city(i), i % 100)?;
    }
    Ok(())
}

fn write_ndjson_set(
    dir: &Path,
    zones: usize,
    customers: usize,
    shipments: usize,
    items: usize,
    depots: usize,
) -> Result<()> {
    let mut w = BufWriter::new(File::create(dir.join("service_zones.ndjson"))?);
    for i in 1..=zones {
        writeln!(w, "{{\"zone_id\":\"z{i}\",\"zone_name\":\"Zone {i}\",\"tier\":\"{}\"}}", tier(i))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("customers.ndjson"))?);
    for i in 1..=customers {
        writeln!(w, "{{\"customer_id\":\"c{i}\",\"zone_id\":\"z{}\",\"status\":\"{}\"}}", 1 + (i % zones), customer_status(i))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("shipments.ndjson"))?);
    for i in 1..=shipments {
        let cid = 1 + (i % customers);
        let zid = 1 + (cid % zones);
        writeln!(w, "{{\"shipment_id\":\"s{i}\",\"customer_id\":\"c{cid}\",\"zone_id\":\"z{zid}\",\"total\":{}}}", 100 + (i % 9000))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("catalog_items.ndjson"))?);
    for i in 1..=items {
        writeln!(w, "{{\"item_id\":\"i{i}\",\"category\":\"cat{}\"}}", 1 + (i % 50))?;
    }

    let mut w = BufWriter::new(File::create(dir.join("customer_items.ndjson"))?);
    for i in 1..=shipments {
        let cid = 1 + (i % customers);
        let iid = 1 + ((i * 17) % items);
        writeln!(w, "{{\"customer_id\":\"c{cid}\",\"item_id\":\"i{iid}\"}}")?;
    }

    let mut w = BufWriter::new(File::create(dir.join("depots.ndjson"))?);
    for i in 1..=depots {
        writeln!(
            w,
            "{{\"depot_id\":\"d{i}\",\"zone_id\":\"z{}\",\"city\":\"{}\",\"load_bucket\":{}}}",
            1 + (i % zones),
            city(i),
            i % 100
        )?;
    }
    Ok(())
}

fn write_bench_files(dir: &Path, format: &str) -> Result<()> {
    let ext = match format {
        "csv" => "csv",
        "tsv" => "tsv",
        _ => "ndjson",
    };

    let bench = format!(
        "--- BENCH ---\nconcurrency: 16\nrequests: 3000\nload_schedule: const\nsources: [{{name: depots, file: ../data/depots.{ext}, format: {format}, indexed_by: zone_id}}, {{name: service_zones, file: ../data/service_zones.{ext}, format: {format}, indexed_by: zone_id}}, {{name: customers, file: ../data/customers.{ext}, format: {format}, indexed_by: zone_id}}, {{name: shipments, file: ../data/shipments.{ext}, format: {format}, indexed_by: customer_id}}, {{name: customer_items, file: ../data/customer_items.{ext}, format: {format}, indexed_by: customer_id}}]\n\n--- ADDRESS ---\nlocalhost:50051\n\n--- ENDPOINT ---\nbench.Service/Call\n\n--- REQUEST ---\n{{\"depot\":\"{{{{depots.depot_id}}}}\",\"zone\":\"{{{{depots.zone_id}}}}\",\"zone_name\":\"{{{{service_zones.zone_name}}}}\",\"customer\":\"{{{{customers.customer_id}}}}\",\"shipment\":\"{{{{shipments.shipment_id}}}}\",\"shipment_customer\":\"{{{{shipments.customer_id}}}}\",\"item\":\"{{{{customer_items.item_id}}}}\",\"item_customer\":\"{{{{customer_items.customer_id}}}}\"}}\n\n--- RESPONSE ---\n{{\"ok\":true}}\n"
    );

    fs::write(dir.join(format!("index_matrix_{format}.gctf")), bench)?;
    Ok(())
}

fn tier(i: usize) -> &'static str {
    match i % 3 {
        0 => "core",
        1 => "edge",
        _ => "remote",
    }
}

fn customer_status(i: usize) -> &'static str {
    if i % 10 == 0 {
        "suspended"
    } else {
        "active"
    }
}

fn city(i: usize) -> &'static str {
    match i % 5 {
        0 => "msk",
        1 => "spb",
        2 => "kzn",
        3 => "ekb",
        _ => "nsk",
    }
}
