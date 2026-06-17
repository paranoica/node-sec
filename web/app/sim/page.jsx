import { SimDashboard } from "./SimDashboard";

// Pure client app; force a static shell so `output: export` prerenders without bailing to dynamic.
export const dynamic = "force-static";

export const metadata = {
  title: "node-sec · simulation control",
  description: "Load-harness control + live SLA telemetry",
};

export default function Page() {
  return <SimDashboard />;
}
