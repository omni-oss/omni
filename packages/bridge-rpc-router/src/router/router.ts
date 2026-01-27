import {
    ResponseStatusCode,
    type Service,
    type ServiceContext,
} from "@omni-oss/bridge-rpc";

export type HandlerContext = ServiceContext;

export type HandlerFn = (context: HandlerContext) => Promise<void>;

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
            return await service.run(context);
        } else {
            const response = await context.response.start(
                ResponseStatusCode.NO_HANDLER_FOR_PATH,
            );
            await response.end();
        }
    }

    public hasService(path: string): boolean {
        return this._services.has(path);
    }
}
