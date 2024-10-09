import { readdir } from 'fs/promises';
import { createReadStream } from 'fs';
import { createInterface } from 'readline';

export async function readJsonFileLineByLine(filePath) {
    var set = new Set();
    const jsonObjects = [];
    const fileStream = createReadStream(filePath);
    const rl = createInterface({
        input: fileStream,
        crlfDelay: Infinity
    });
    for await (const line of rl) {
        if (line.trim()) {
        try {
            const obj = JSON.parse(line);
            const mangled =obj.s + "::" + obj.o;
            if(set.has(mangled)) {
                // Pass
            } else {
                set.add(mangled)
                jsonObjects.push(obj);
            }
        } catch (error) {
            console.error(`Error parsing line as JSON: ${line}`, error);
        }
        }
    }
    return jsonObjects;
}

export async function GET() {
    const currentDir = process.cwd();
    const graph = await readJsonFileLineByLine(currentDir + '/.next/openship/graph');
    const runtime = {
        name: process.release.name,
        version: process.version,
    };
    const framework = {
        name: 'nextjs'
    };
    const data = {
        runtime,
        framework,
        openship: {
            version: 'beta',
            protocols: ["osh1"],
        },
        application: {
            name: '{application}',
            version: '{version}',
            graph
        },
    };
    return Response.json(data);
}