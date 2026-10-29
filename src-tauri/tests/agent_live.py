"""Exercise an actual Claude Code client against an isolated Hissabeth workspace.

Uses the existing executable; no build, personal MCP registration, or desktop
workspace modification. Artifacts and the complete client transcript remain in
the existing target directory. A passing run requires observed MCP calls and
independent saved-model checks, not the agent's final prose.
"""
import argparse
import json
import math
import os
from pathlib import Path
import select
import subprocess
import time


ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hissabeth"
RUN = ROOT / "target" / f"agent-live-{time.time_ns()}"
NAME = "Live agent BK7 experiment"
TOOLS = ["workspace_status", "capabilities", "system_get", "system_set",
         "autofocus", "paraxial", "spot_diagram", "project_export"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", help="Explicit Claude Code model ID; otherwise use its configured model")
    parser.add_argument("--bench", action="store_true", help="Evaluate free-space bench creation, obstruction diagnosis and repair")
    parser.add_argument("--bench-contract", action="store_true", help="Evaluate mount JSON/MCP fields without repeating optical scenarios")
    options = parser.parse_args()
    RUN.mkdir()
    env = dict(os.environ, HISSABETH_DATA_DIR=str(RUN))
    config = {"mcpServers": {"hissabeth": {
        "command": str(BINARY), "args": ["--mcp"],
        "env": {"HISSABETH_DATA_DIR": str(RUN)},
    }}}
    config_path = RUN / "mcp.json"
    config_path.write_text(json.dumps(config, indent=2) + "\n")
    prompt = f"""You are testing a real optical design application through its MCP server.
Use only the provided Hissabeth MCP tools. Do not simulate results.
1. Read workspace_status, capabilities, and system_get.
2. Save the singlet as {NAME!r}: first surface radius 60 mm, second
surface radius -60 mm, pupil diameter 4 mm. Retain all other prescription
data. Use system_set with the revision you read.
3. Repeat that system_set once using the OLD revision; confirm
the stale write is rejected. Read the current model before continuing.
4. Autofocus at 587.5618 nm with the CURRENT revision. Read system_get again.
5. Run paraxial at 587.5618 nm and spot_diagram for field_index 0 at
587.5618 nm with sampling {{"kind":"grid","count":9}}.
6. Export the saved project with format hissabeth and confirm its name.
7. From capabilities explain whether tilted mirrors, beam splitters and
multiple branches can currently be modeled. Do not claim unsupported tools
exist. Report actual focal length, final gap, RMS spot, and revisions.
Your final response must be concise Simplified Chinese.
"""
    required_tools = TOOLS
    if options.bench:
        required_tools = ["workspace_status", "capabilities", "system_get", "bench_get", "bench_example", "bench_set", "bench_trace", "bench_assembly", "project_export"]
        prompt = """Test the actual free-space laser bench MCP workflow. Use only the provided tools; do not simulate results.
Read workspace_status, capabilities, system_get and bench_get. Get bench_example.
Save that example as 'Live agent split laser bench' using bench_set and the current shared revision. Set BS1 reflectance to 0.3 and transmittance to 0.7. Keep all other parameters.
Repeat that save ONCE using the OLD revision and confirm it is rejected.
Read bench_get. Trace the saved bench. Confirm D1 receives 0.7 and D2 receives 0.3 source power (sum the individual packets).
Create an obstruction failure by setting M1 Z to 175 mm and save with the CURRENT revision. Trace the saved bench; identify the failed reflected branch from the actual diagnostics.
Read bench_get again, restore M1 Z to 75 mm and save with the CURRENT revision. Trace the saved bench to confirm both detectors receive power again.
Read bench_assembly and export project_export format hissabeth. Read system_get to confirm the bench is stored in the project.
Report actual tool results and power accounting, including model limitations. Do not invoke sequential analyses for the bench. Concise Simplified Chinese.
"""
    if options.bench_contract:
        required_tools = ["workspace_status", "system_get", "bench_get", "bench_example", "bench_set", "bench_trace", "bench_assembly", "project_export"]
        prompt = """Test the live Hissabeth MCP mount-data contract using only provided tools, with no simulated results.
Read workspace_status, system_get and bench_get. Get bench_example. Save it with name 'Live agent mount contract', changing only BS1.mount.base_size to [42,36] mm. Keep the optics and other data unchanged; use the current revision.
Repeat that save once with the OLD revision and confirm rejection. Read bench_get again. Trace the saved bench and confirm total detected power is 1. Read bench_assembly and confirm BS1 mount base_size is [42,36]. Export the native project (project_export format hissabeth) and read system_get to verify storage. Report the actual saved mount dimensions and power. Concise Simplified Chinese.
"""
    (RUN / "prompt.txt").write_text(prompt)
    command = ["claude", "--bare", "-p",
               "--no-session-persistence", "--strict-mcp-config",
               "--mcp-config", str(config_path), "--tools", "",
               "--allowedTools", *[f"mcp__hissabeth__{name}" for name in required_tools],
               "--permission-mode", "dontAsk", "--output-format", "stream-json",
               "--verbose", "--max-turns", "36"]
    if options.model:
        command.extend(["--model", options.model])
    command.extend(["--", prompt])
    engine = relay = None
    try:
        with (RUN / "engine.stderr").open("w") as err:
            engine = subprocess.Popen([str(BINARY), "--engine"], env=env,
                                      stdout=subprocess.DEVNULL, stderr=err)
        for _ in range(100):
            if engine.poll() is not None:
                raise RuntimeError("Engine exited; see engine.stderr")
            if (RUN / "endpoint.json").exists():
                break
            time.sleep(.05)
        else:
            raise TimeoutError("Engine did not publish an endpoint")
        with (RUN / "client.jsonl").open("w") as out, (RUN / "client.stderr").open("w") as err:
            result = subprocess.run(command, env=env, cwd=ROOT.parent,
                                    stdout=out, stderr=err, timeout=420)
        events = [json.loads(line) for line in (RUN / "client.jsonl").read_text().splitlines() if line.strip()]
        calls = [block for event in events if event.get("type") == "assistant"
                 for block in event.get("message", {}).get("content", [])
                 if block.get("type") == "tool_use"]
        summaries = [event for event in events if event.get("type") == "result"]
        models = [event.get("model") for event in events if event.get("type") == "system" and event.get("subtype") == "init"]
        (RUN / "client-summary.json").write_text(json.dumps({
            "returncode": result.returncode, "models": models,
            "calls": calls, "result": summaries,
        }, ensure_ascii=False, indent=2) + "\n")
        assert result.returncode == 0, "Client failed; see client-summary.json and client.stderr"
        assert summaries and not summaries[-1].get("is_error"), "Agent did not finish successfully"
        names = [call["name"].removeprefix("mcp__hissabeth__") for call in calls]
        assert set(required_tools) <= set(names), f"Missing actual MCP calls: {set(required_tools) - set(names)}"
        writes = [call for call in calls if call["name"] == ("mcp__hissabeth__bench_set" if (options.bench or options.bench_contract) else "mcp__hissabeth__system_set")]
        assert len(writes) == (4 if options.bench else 2) and writes[0]["input"]["revision"] == writes[1]["input"]["revision"], "Missing deliberate stale write"
        tool_results = [block for event in events if event.get("type") == "user"
                        for block in event.get("message", {}).get("content", [])
                        if isinstance(block, dict) and block.get("type") == "tool_result"]
        assert any(block.get("tool_use_id") == writes[1]["id"] and block.get("is_error") for block in tool_results), "No observed stale-write rejection"
        relay = subprocess.Popen([str(BINARY), "--mcp"], env=env, stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        counter = 0

        def rpc(method, params):
            nonlocal counter
            counter += 1
            relay.stdin.write(json.dumps(dict(jsonrpc="2.0", id=counter, method=method, params=params)) + "\n")
            relay.stdin.flush()
            assert select.select([relay.stdout], [], [], 20)[0], "Verification relay timed out"
            response = json.loads(relay.stdout.readline())
            assert response["id"] == counter and "error" not in response, response
            return response["result"]

        def tool(name, args=None):
            response = rpc("tools/call", dict(name=name, arguments=args or {}))
            assert not response.get("isError"), response
            return json.loads(response["content"][0]["text"])

        rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                           "clientInfo": {"name": "live-agent-verifier", "version": "1"}})
        snapshot = tool("system_get")
        system = snapshot["system"]
        if options.bench_contract:
            scene = system["bench"]
            assert scene["name"] == "Live agent mount contract"
            first = next(e for e in scene["elements"] if e["id"] == "BS1")
            assert first["mount"]["base_size"] == [42,36]
            assert snapshot["revision"] == writes[0]["input"]["revision"] + 1
            trace = tool("bench_trace")
            assert abs(trace["power"]["detected"] - 1) < 1e-9
            report = tool("bench_assembly")
            assert next(e for e in report["items"] if e["id"] == "BS1")["mount"]["base_size"] == [42,36]
            exported = tool("project_export", {"format":"hissabeth"})
            assert json.loads(exported["text"]) == system
            assert json.loads((RUN / "system.json").read_text()) == system
            evidence = {"status":"passed", "scenario":"mount JSON/MCP contract", "workspace":str(RUN), "models":models,
                        "actual_tool_calls":names, "snapshot":snapshot, "trace":trace, "assembly":report, "agent_result":summaries[-1]}
        elif options.bench:
            scene = system["bench"]
            assert scene["name"] == "Live agent split laser bench"
            assert next(e for e in scene["elements"] if e["id"] == "M1")["position"][2] == 75
            assert snapshot["revision"] == writes[0]["input"]["revision"] + 3
            trace = tool("bench_trace")
            assert abs(trace["power"]["detected"] - 1) < 1e-9
            assert abs(trace["balance_error"]) < 1e-9
            for detector, expected in [("D1", .7), ("D2", .3)]:
                assert abs(sum(t["power"] for t in trace["terminations"] if t["element"] == detector and t["reason"] == "detected") - expected) < 1e-9
            observed_traces = []
            trace_ids = {c["id"] for c in calls if c["name"] == "mcp__hissabeth__bench_trace"}
            for block in tool_results:
                if block.get("tool_use_id") not in trace_ids:
                    continue
                content = block.get("content", [])
                text = content if isinstance(content, str) else "".join(b.get("text", "") for b in content)
                observed_traces.append(json.loads(text))
            damaged = [t for t in observed_traces if t["revision"] == writes[0]["input"]["revision"] + 2]
            assert damaged and damaged[-1]["power"]["blocked"] + damaged[-1]["power"]["escaped"] > .29
            assert abs(damaged[-1]["power"]["detected"] - .7) < 1e-9
            blocked = writes[2]["input"]["bench"]
            assert next(e for e in blocked["elements"] if e["id"] == "M1")["position"][2] == 175
            assert len([c for c in calls if c["name"] == "mcp__hissabeth__bench_trace"]) >= 3
            report = tool("bench_assembly")
            assert len(report["items"]) == 5 and len(report["connections"]) == 5
            exported = tool("project_export", {"format": "hissabeth"})
            assert json.loads(exported["text"]) == system
            assert json.loads((RUN / "system.json").read_text()) == system
            evidence = {"status": "passed", "scenario": "free-space bench", "workspace": str(RUN), "models": models,
                        "actual_tool_calls": names, "snapshot": snapshot, "trace": trace,
                        "assembly": report, "agent_result": summaries[-1]}
        else:
            assert system["name"] == NAME
            assert [surface["radius"] for surface in system["surfaces"]] == [60, -60]
            assert system["pupil"]["value"] == 4
            assert snapshot["revision"] == writes[0]["input"]["revision"] + 2
            paraxial = tool("paraxial", {"wavelength": 587.5618})
            assert math.isfinite(paraxial["efl"]) and 50 < paraxial["efl"] < 70
            assert abs(system["surfaces"][-1]["thickness"] - paraxial["image_distance"]) < 1e-8
            spot = tool("spot_diagram", {"field_index": 0, "wavelength": 587.5618,
                                         "sampling": {"kind": "grid", "count": 9}})
            exported = tool("project_export", {"format": "hissabeth"})
            assert json.loads(exported["text"]) == system
            assert json.loads((RUN / "system.json").read_text()) == system
            evidence = {"status": "passed", "workspace": str(RUN), "models": models,
                        "actual_tool_calls": names, "snapshot": snapshot,
                        "paraxial": paraxial, "spot": spot,
                        "agent_result": summaries[-1]}
        (RUN / "evidence.json").write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps({"status": "passed", "evidence": str(RUN / "evidence.json")}, ensure_ascii=False), flush=True)
    finally:
        for process in (relay, engine):
            if process is not None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
        print(f"Artifacts: {RUN}", flush=True)


if __name__ == "__main__":
    main()
