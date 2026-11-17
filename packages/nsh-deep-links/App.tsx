import { mount, signal, Text, View } from "nsh";
import { DeepLinks, DeepLinksError, RouteTable, memoryBackend } from "./src";

/** The demo runs on a memory backend, so the entry never waits on a platform: the press plays
 * the system and delivers one link, which the waiting `next` resolves with. */
const backend = memoryBackend();
const links = new DeepLinks(
  backend,
  RouteTable.fromConfig({
    prefixes: ["https://example.com", "myapp://"],
    screens: { Order: { path: "orders/:id", parse: { id: "integer" } } },
  }),
);

const message = signal("no link yet");

function codeOf(error: unknown): string {
  return error instanceof DeepLinksError ? error.code : "unknown error";
}

function open(url: string) {
  message.set("waiting");
  links.next().then(
    (link) => message.set(`${link.kind} ${link.route.screen} ${JSON.stringify(link.route.params)}`),
    (error: unknown) => message.set(codeOf(error)),
  );
  backend.deliver(url);
}

function App() {
  return (
    <View class="flex-1 gap-3 p-4">
      <Text testID="title" class="text-foreground">nsh-deep-links</Text>
      <Text testID="message" class="text-foreground">{message()}</Text>
      <Text testID="open" role="button" onPress={() => open("myapp://orders/42")} class="rounded-(--radius) bg-muted px-3 py-2">
        Open myapp://orders/42
      </Text>
      <Text testID="unclaimed" role="button" onPress={() => open("other://orders/42")} class="rounded-(--radius) bg-muted px-3 py-2">
        Open other://orders/42
      </Text>
    </View>
  );
}

mount(App);
