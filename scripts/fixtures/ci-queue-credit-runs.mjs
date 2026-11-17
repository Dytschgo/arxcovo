// Public attempt-1 metadata only; read-only GitHub API, 2026-11-16. No logs or steps.
export const actualRuns = [
  {
    "run": {
      "id": 43880904887,
      "head_sha": "c6121523417089bc0afaf307470183dd4a7f47ba",
      "run_attempt": 1,
      "path": ".github/workflows/ci.yml",
      "event": "push",
      "head_branch": "main",
      "status": "completed",
      "conclusion": "failure",
      "run_started_at": "2026-11-16T13:58:02Z",
      "updated_at": "2026-11-16T14:22:37Z",
      "head_repository": {
        "id": 1453422996
      }
    },
    "jobs": {
      "total_count": 3,
      "jobs": [
        {
          "id": 133346204427,
          "run_id": 43880904887,
          "run_attempt": 1,
          "head_sha": "c6121523417089bc0afaf307470183dd4a7f47ba",
          "name": "verify (windows-latest)",
          "labels": [
            "windows-latest"
          ],
          "status": "completed",
          "conclusion": "failure",
          "created_at": "2026-11-16T13:58:03Z",
          "started_at": "2026-11-16T13:59:12Z",
          "completed_at": "2026-11-16T14:05:24Z"
        },
        {
          "id": 133346205159,
          "run_id": 43880904887,
          "run_attempt": 1,
          "head_sha": "c6121523417089bc0afaf307470183dd4a7f47ba",
          "name": "verify (macos-latest)",
          "labels": [
            "macos-latest"
          ],
          "status": "completed",
          "conclusion": "success",
          "created_at": "2026-11-16T13:58:03Z",
          "started_at": "2026-11-16T14:17:04Z",
          "completed_at": "2026-11-16T14:22:37Z"
        },
        {
          "id": 133346205218,
          "run_id": 43880904887,
          "run_attempt": 1,
          "head_sha": "c6121523417089bc0afaf307470183dd4a7f47ba",
          "name": "verify (ubuntu-latest)",
          "labels": [
            "ubuntu-latest"
          ],
          "status": "completed",
          "conclusion": "success",
          "created_at": "2026-11-16T13:58:03Z",
          "started_at": "2026-11-16T13:58:40Z",
          "completed_at": "2026-11-16T14:02:28Z"
        }
      ]
    }
  },
  {
    "run": {
      "id": 43889323450,
      "head_sha": "bf86f4a4dd4f16849022f23c3a61b35be3f98bef",
      "run_attempt": 1,
      "path": ".github/workflows/ci.yml",
      "event": "pull_request",
      "head_branch": "work/m3-r5-postrelease-observation",
      "status": "completed",
      "conclusion": "success",
      "run_started_at": "2026-11-16T14:46:43Z",
      "updated_at": "2026-11-16T15:08:34Z",
      "head_repository": {
        "id": 1453422996
      }
    },
    "jobs": {
      "total_count": 3,
      "jobs": [
        {
          "id": 133375899952,
          "run_id": 43889323450,
          "run_attempt": 1,
          "head_sha": "bf86f4a4dd4f16849022f23c3a61b35be3f98bef",
          "name": "verify (windows-latest)",
          "labels": [
            "windows-latest"
          ],
          "status": "completed",
          "conclusion": "success",
          "created_at": "2026-11-16T14:46:44Z",
          "started_at": "2026-11-16T14:50:22Z",
          "completed_at": "2026-11-16T15:00:07Z"
        },
        {
          "id": 133375900319,
          "run_id": 43889323450,
          "run_attempt": 1,
          "head_sha": "bf86f4a4dd4f16849022f23c3a61b35be3f98bef",
          "name": "verify (ubuntu-latest)",
          "labels": [
            "ubuntu-latest"
          ],
          "status": "completed",
          "conclusion": "success",
          "created_at": "2026-11-16T14:46:44Z",
          "started_at": "2026-11-16T14:48:03Z",
          "completed_at": "2026-11-16T14:51:51Z"
        },
        {
          "id": 133375900358,
          "run_id": 43889323450,
          "run_attempt": 1,
          "head_sha": "bf86f4a4dd4f16849022f23c3a61b35be3f98bef",
          "name": "verify (macos-latest)",
          "labels": [
            "macos-latest"
          ],
          "status": "completed",
          "conclusion": "success",
          "created_at": "2026-11-16T14:46:44Z",
          "started_at": "2026-11-16T15:06:02Z",
          "completed_at": "2026-11-16T15:08:33Z"
        }
      ]
    }
  }
];
