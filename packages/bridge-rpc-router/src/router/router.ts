import {
    ResponseStatusCode,
    type Service,
    type ServiceContext,
} from "@omni-oss/bridge-rpc-core";

export type HandlerContext = ServiceContext;

export type HandlerFn = (context: HandlerContext) => Promise<void>;

const textEncoder = new TextEncoder();
export class Router implements Service {
    private _services: Map<string, Service> = new Map();

    public addService(path: string, service: Service): Router {
        this._services.set(path, service);
        return this;
    }

    public addHandler(path: string, handler: HandlerFn): Router {
        return this.addService(path, {
            run: handler,
        });
    }

    public async run(context: ServiceContext): Promise<void> {
        const path = context.request.path;
        const service = this._services.get(path);

        if (service) {
            try {
                return await service.run(context);
            } catch (err) {
                const errString =
                    err instanceof Error
                        ? err.stack || err.message
                        : String(err);

                if (!context.response.isStarted) {
                    await using response = await context.response.start(
                        ResponseStatusCode.from(500),
                    );
                    await response.writeBodyChunk(
                        textEncoder.encode(errString),
                    );
                    await response.end();
                }
            }
        } else {
            await using response = await context.response.start(
                ResponseStatusCode.NO_HANDLER_FOR_PATH,
            );
            await response.end();
        }
    }

    public hasService(path: string): boolean {
        return this._services.has(path);
    }
}
