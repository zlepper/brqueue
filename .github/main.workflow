workflow "Publish" {
  on = "push"
  resolves = ["Push docker image"]
}

action "Login" {
  uses = "actions/docker/login@76ff57a"
  secrets = ["DOCKER_USERNAME", "DOCKER_PASSWORD"]
}

action "Build docker image" {
  uses = "actions/docker/cli@76ff57a"
  args = "build . -t zlepper/brqueue:latest"
}

action "Push docker image" {
  uses = "actions/docker/cli@76ff57a"
  needs = ["Login", "Build docker image"]
  args = "push zlepper/brqueue:latest"
}
