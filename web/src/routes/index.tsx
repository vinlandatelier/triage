import { createFileRoute } from "@tanstack/react-router";
import { HelloPage } from "@/features/hello/hello-page";
import { helloListOptions } from "@/features/hello/queries";

export const Route = createFileRoute("/")({
  loader: ({ context }) => context.queryClient.ensureQueryData(helloListOptions()),
  component: HelloPage,
});
