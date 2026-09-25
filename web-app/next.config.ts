import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  agentRules: false,
  // Docker image copies .next/standalone + .next/static + public (see ops/docker/web-app.Dockerfile).
  output: "standalone",
};

export default nextConfig;
