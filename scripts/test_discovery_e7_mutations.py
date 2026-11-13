"""E7 compiled regression controls; fixed labels only, exact byte restoration."""
from pathlib import Path
import subprocess
A=Path('crates/arxd/src/service/application.rs')
S=Path('crates/arxd/src/store.rs')
original={p:p.read_bytes() for p in (A,S)}
mutants=[
 ('dispatch_before_ready',A,'if !blocking.run(move || Ok(setup(setup_stop))).await?', 'if false && !blocking.run(move || Ok(setup(setup_stop))).await?', 'w1_w4_t20b_order_single_domain'),
 ('row_on_setup_failure',A,'if !blocking.run(move || Ok(setup(setup_stop))).await? {\n                    return Ok(());\n                }','if !blocking.run(move || Ok(setup(setup_stop))).await? {\n                    // mutant continues to row\n                }','w2_w3_shutdown_boundaries'),
 ('shutdown_after_ready_ignored',A,'if stop.is_cancelled() {','if false {','w2_w3_shutdown_boundaries'),
 ('cache_row_k',S,'if response["outcome"] != "succeeded" {','if response["outcome"] == "unreachable" {','w5_only_success_response_and_strict_tickets'),
 ('completion_order_overwrite',S,'observation.0 > c.ticket','{ let _ = c; true }','w5_only_success_response_and_strict_tickets'),
 ('identity_metadata_bypassed',S,'observed_identity: response["observed_identity"].clone(),','observed_identity: serde_json::json!({"id":"unredacted-evidence-marker"}),','w6_w7_mcp_cache_and_no_cache_golden'),
 ('production_start_discovery_spawn',A,'// No build-account backend: never probe the privileged service\'s PATH.','// No build-account backend: never probe the privileged service\'s PATH.\n        let _task = tokio::spawn(async {});','w8_production_and_domain_source_pins'),
]
try:
 for name,path,before,after,test in mutants:
  source=original[path].decode().replace('\r\n','\n')
  assert before in source, 'anchor required'
  path.write_text(source.replace(before,after,1))
  result=subprocess.run(['cargo','test','-p','arxd','--lib','--locked',test],capture_output=True,timeout=120)
  output=result.stdout+result.stderr
  compiled=b'error[E' not in output and b'could not compile' not in output
  killed=result.returncode!=0 and (test+' ... FAILED').encode() in output
  print(name+(': COMPILED KILLED' if compiled and killed else ': NOT PROVEN'),flush=True)
  assert compiled and killed,'compiled assertion kill required'
  path.write_bytes(original[path])
finally:
 for path,content in original.items(): path.write_bytes(content)
 assert all(p.read_bytes()==b for p,b in original.items()),'byte restore'
 print('source: EXACT RESTORE',flush=True)
