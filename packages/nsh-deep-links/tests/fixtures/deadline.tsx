import { mount, signal, Text, View } from "nsh";
import { DeepLinks, DeepLinksError, RouteTable, memoryBackend } from "../../src";

/** A wait with a 30 s deadline, on whatever clock the test gives the app. */
const backend = memoryBackend();
const links = new DeepLinks(
  backend,
  RouteTable.fromConfig({ prefixes: ["myapp://"], screens: { Chat: "chat/:author" } }),
);
const answer = signal("idle");

function wait() {
  answer.set("waiting");
  links.nextWithin(30_000).then(
    (link) => answer.set(`link ${link.route.screen}`),
    (error: unknown) => answer.set(error instanceof DeepLinksError ? error.code : "unknown"),
  );
}

function App() {
  return (
    <View class="flex-1 gap-3 p-4">
      <Text testID="answer" class="text-foreground">{answer()}</Text>
      <Text testID="wait" role="button" onPress={wait} class="px-3 py-2">Wait</Text>
      <Text testID="deliver" role="button" onPress={() => backend.deliver("myapp://chat/jane")} class="px-3 py-2">
        Deliver
      </Text>
    </View>
  );
}

mount(App);
